CREATE TABLE IF NOT EXISTS entitlements (
  install_id TEXT PRIMARY KEY,
  customer_id TEXT,
  lifetime INTEGER NOT NULL DEFAULT 0,
  subscription_id TEXT,
  subscription_status TEXT,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_entitlements_customer_id ON entitlements(customer_id);
CREATE INDEX IF NOT EXISTS idx_entitlements_subscription_id ON entitlements(subscription_id);
