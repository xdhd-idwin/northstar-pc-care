# Northstar billing Worker (Sandbox)

This Cloudflare Worker keeps Stripe's secret key off the desktop app. It creates Checkout Sessions for the monthly and one-time Price IDs, verifies Stripe webhook signatures, and stores Pro entitlements in Cloudflare D1. The supplied Price IDs are configured in `wrangler.toml`.

## Deploy it

1. Create a Cloudflare account and install a current Node.js release supported by Wrangler. Cloudflare's current Workers Free plan includes 100,000 requests per day; D1 has separate daily read/write and storage limits, so monitor usage as the user base grows.
2. In PowerShell, go to this folder and run:

   ```powershell
   pnpm install
   pnpm wrangler login
   pnpm wrangler d1 create northstar-billing
   ```

3. Wrangler prints a database ID. Replace `REPLACE_WITH_ID_FROM_WRANGLER_D1_CREATE` in `wrangler.toml` with that ID. Initialize the remote database:

   ```powershell
   pnpm wrangler d1 execute northstar-billing --remote --file=schema.sql
   ```

4. Deploy once to create the Worker URL:

   ```powershell
   pnpm run deploy
   ```

5. In the Stripe Dashboard, stay in Sandbox. Get the Sandbox secret API key from **Developers → API keys**. Add it to the Worker as a secret; the command prompts locally so you don't have to paste it into chat or source files:

   ```powershell
   pnpm wrangler secret put STRIPE_SECRET_KEY
   ```

6. In Stripe **Developers → Webhooks**, add `https://YOUR-WORKER-URL/webhook` and subscribe to `checkout.session.completed`, `checkout.session.async_payment_succeeded`, `customer.subscription.created`, `customer.subscription.updated`, and `customer.subscription.deleted`. Copy that endpoint's signing secret into the Worker:

   ```powershell
   pnpm wrangler secret put STRIPE_WEBHOOK_SECRET
   ```

7. Deploy again with `pnpm run deploy`. Verify `https://YOUR-WORKER-URL/health` returns `{"ok":true}`. Put the Worker HTTPS URL in `electron/billing-source.json`, for example `{"baseUrl":"https://northstar-billing.YOUR-SUBDOMAIN.workers.dev"}`, then rebuild Northstar.

Cloudflare stores these values as encrypted Worker secrets; they are not committed to Git. Stripe's own key guidance says to store sensitive keys in a hosting secrets vault and keep them out of source code. See [Cloudflare Worker secrets](https://developers.cloudflare.com/workers/configuration/secrets/) and [Stripe API key safety](https://docs.stripe.com/keys).

The checkout success/cancel page currently returns to the Northstar GitHub repository; switch back to the desktop app after checkout and it will poll entitlement status. Sandbox prices and test keys cannot charge real money and cannot be used in live mode. Before real sales, create live-mode products/prices and configure live keys/webhook secrets, then review tax, consumer, privacy, security, refund and subscription cancellation requirements. This initial licensing service is a small first-party implementation, not a tamper-proof DRM system.
