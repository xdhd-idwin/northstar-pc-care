const installationId = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
const encoder = new TextEncoder();

function json(data, status = 200) {
  return new Response(JSON.stringify(data), { status, headers: { 'content-type': 'application/json; charset=utf-8', 'cache-control': 'no-store' } });
}
function secureEqual(a, b) {
  if (a.length !== b.length) return false;
  let difference = 0;
  for (let i = 0; i < a.length; i++) difference |= a.charCodeAt(i) ^ b.charCodeAt(i);
  return difference === 0;
}
function hex(bytes) { return [...new Uint8Array(bytes)].map((byte) => byte.toString(16).padStart(2, '0')).join(''); }
async function verifyStripeSignature(rawBody, header, secret) {
  if (!header || !secret) return false;
  const parts = Object.fromEntries(header.split(',').map((part) => part.split('=', 2)));
  const timestamp = Number(parts.t);
  if (!Number.isFinite(timestamp) || Math.abs(Date.now() / 1000 - timestamp) > 300 || !parts.v1) return false;
  const key = await crypto.subtle.importKey('raw', encoder.encode(secret), { name: 'HMAC', hash: 'SHA-256' }, false, ['sign']);
  const signature = hex(await crypto.subtle.sign('HMAC', key, encoder.encode(`${parts.t}.${rawBody}`)));
  return secureEqual(signature, parts.v1);
}
async function stripePost(env, endpoint, values) {
  const response = await fetch(`https://api.stripe.com/v1/${endpoint}`, {
    method: 'POST',
    headers: { authorization: `Bearer ${env.STRIPE_SECRET_KEY}`, 'content-type': 'application/x-www-form-urlencoded' },
    body: new URLSearchParams(values)
  });
  const data = await response.json();
  if (!response.ok) {
    console.error('Stripe API error:', response.status, data?.error?.type || 'unknown');
    throw new Error('Stripe could not create Checkout. Check the Sandbox service configuration.');
  }
  return data;
}
async function customerForInstall(env, installId) {
  const row = await env.DB.prepare('SELECT customer_id FROM entitlements WHERE install_id = ?').bind(installId).first();
  if (row?.customer_id) return row.customer_id;
  const customer = await stripePost(env, 'customers', { 'metadata[northstar_install_id]': installId });
  await env.DB.prepare('INSERT INTO entitlements (install_id, customer_id) VALUES (?, ?) ON CONFLICT(install_id) DO UPDATE SET customer_id = excluded.customer_id, updated_at = CURRENT_TIMESTAMP').bind(installId, customer.id).run();
  return customer.id;
}
async function createCheckout(request, env) {
  let body;
  try { body = await request.json(); } catch { return json({ error: 'Invalid checkout request.' }, 400); }
  const { installId, plan } = body || {};
  if (typeof installId !== 'string' || !installationId.test(installId) || !['monthly', 'lifetime'].includes(plan)) return json({ error: 'Invalid checkout request.' }, 400);
  try {
    const customerId = await customerForInstall(env, installId);
    const price = plan === 'monthly' ? env.STRIPE_PRICE_MONTHLY : env.STRIPE_PRICE_LIFETIME;
    const values = {
      mode: plan === 'monthly' ? 'subscription' : 'payment',
      customer: customerId,
      client_reference_id: installId,
      success_url: env.CHECKOUT_SUCCESS_URL,
      cancel_url: env.CHECKOUT_CANCEL_URL,
      'line_items[0][price]': price,
      'line_items[0][quantity]': '1',
      'metadata[northstar_install_id]': installId,
      'metadata[northstar_plan]': plan
    };
    if (plan === 'monthly') {
      values['subscription_data[metadata][northstar_install_id]'] = installId;
      values['subscription_data[metadata][northstar_plan]'] = plan;
    } else {
      values['payment_intent_data[metadata][northstar_install_id]'] = installId;
      values['payment_intent_data[metadata][northstar_plan]'] = plan;
    }
    const session = await stripePost(env, 'checkout/sessions', values);
    return json({ url: session.url });
  } catch (error) {
    console.error('Checkout creation failed:', error.message);
    return json({ error: error.message || 'Could not create Stripe Checkout.' }, 502);
  }
}
async function updateFromEvent(event, env) {
  const object = event.data?.object;
  if (!object) return;
  if (event.type === 'checkout.session.completed' || event.type === 'checkout.session.async_payment_succeeded') {
    const installId = object.metadata?.northstar_install_id || object.client_reference_id;
    if (!installId || !installationId.test(installId)) return;
    const customerId = typeof object.customer === 'string' ? object.customer : object.customer?.id;
    if (object.mode === 'payment' && object.payment_status === 'paid') {
      await env.DB.prepare('INSERT INTO entitlements (install_id, customer_id, lifetime) VALUES (?, ?, 1) ON CONFLICT(install_id) DO UPDATE SET customer_id = excluded.customer_id, lifetime = 1, updated_at = CURRENT_TIMESTAMP').bind(installId, customerId || null).run();
    } else if (object.mode === 'subscription' && object.subscription) {
      const subscriptionId = typeof object.subscription === 'string' ? object.subscription : object.subscription.id;
      await env.DB.prepare('INSERT INTO entitlements (install_id, customer_id, subscription_id, subscription_status) VALUES (?, ?, ?, NULL) ON CONFLICT(install_id) DO UPDATE SET customer_id = excluded.customer_id, subscription_id = excluded.subscription_id, updated_at = CURRENT_TIMESTAMP').bind(installId, customerId || null, subscriptionId).run();
    }
    return;
  }
  if (event.type.startsWith('customer.subscription.')) {
    const installId = object.metadata?.northstar_install_id;
    if (!installId || !installationId.test(installId)) return;
    const customerId = typeof object.customer === 'string' ? object.customer : object.customer?.id;
    await env.DB.prepare('INSERT INTO entitlements (install_id, customer_id, subscription_id, subscription_status) VALUES (?, ?, ?, ?) ON CONFLICT(install_id) DO UPDATE SET customer_id = excluded.customer_id, subscription_id = excluded.subscription_id, subscription_status = excluded.subscription_status, updated_at = CURRENT_TIMESTAMP').bind(installId, customerId || null, object.id, object.status).run();
  }
}
async function handleWebhook(request, env) {
  const rawBody = await request.text();
  if (!await verifyStripeSignature(rawBody, request.headers.get('stripe-signature'), env.STRIPE_WEBHOOK_SECRET)) return new Response('Invalid Stripe webhook signature.', { status: 400 });
  let event;
  try { event = JSON.parse(rawBody); } catch { return new Response('Invalid event body.', { status: 400 }); }
  try { await updateFromEvent(event, env); return json({ received: true }); }
  catch (error) { console.error('Webhook processing failed:', error.message); return json({ error: 'Webhook processing failed.' }, 500); }
}
async function getEntitlement(installId, env) {
  if (!installationId.test(installId)) return json({ error: 'Invalid installation.' }, 400);
  const row = await env.DB.prepare('SELECT lifetime, subscription_status FROM entitlements WHERE install_id = ?').bind(installId).first();
  const pro = row?.lifetime === 1 || ['active', 'trialing'].includes(row?.subscription_status);
  return json({ pro, plan: pro ? (row.lifetime === 1 ? 'lifetime' : 'monthly') : null });
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    if (request.method === 'GET' && url.pathname === '/health') return json({ ok: true });
    if (request.method === 'POST' && url.pathname === '/webhook') return handleWebhook(request, env);
    if (request.method === 'POST' && url.pathname === '/v1/checkout') return createCheckout(request, env);
    const match = url.pathname.match(/^\/v1\/entitlement\/([^/]+)$/);
    if (request.method === 'GET' && match) {
      try { return await getEntitlement(decodeURIComponent(match[1]), env); }
      catch (error) { console.error('Entitlement lookup failed:', error.message); return json({ error: 'Could not check Pro status.' }, 500); }
    }
    return json({ error: 'Not found.' }, 404);
  }
};
