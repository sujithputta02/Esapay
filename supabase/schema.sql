-- ==============================================================================
-- ESAPay - Supabase Production Schema & API Key Ledger
-- ==============================================================================
-- Provides multi-tenant merchant identity, enterprise API key management,
-- SHA-256 cryptographic key verification, and Row-Level Security (RLS).
-- ==============================================================================

-- 1. Merchants / Organizations Table (Linked to Supabase Auth)
CREATE TABLE IF NOT EXISTS public.merchants (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID REFERENCES auth.users(id) ON DELETE CASCADE,
    email TEXT NOT NULL,
    organization_name TEXT NOT NULL DEFAULT 'Default Workspace',
    tier TEXT NOT NULL DEFAULT 'developer' CHECK (tier IN ('developer', 'startup', 'enterprise')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Index for fast user lookup
CREATE INDEX IF NOT EXISTS idx_merchants_user_id ON public.merchants(user_id);

-- 2. API Keys Ledger Table
CREATE TABLE IF NOT EXISTS public.api_keys (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    merchant_id UUID REFERENCES public.merchants(id) ON DELETE CASCADE,
    name TEXT NOT NULL DEFAULT 'Production Key',
    key_prefix TEXT NOT NULL, -- First 16 characters for identification (e.g. esa_live_sec_7a9f...)
    key_hash TEXT NOT NULL UNIQUE, -- Cryptographic SHA-256 hex hash of the raw secret
    environment TEXT NOT NULL CHECK (environment IN ('test', 'live', 'sandbox')),
    scopes TEXT[] NOT NULL DEFAULT ARRAY['payments:write', 'telemetry:read', 'gateways:toggle'],
    is_active BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ
);

-- Indexes for lightning-fast hash lookup by the Axum backend
CREATE INDEX IF NOT EXISTS idx_api_keys_hash ON public.api_keys(key_hash);
CREATE INDEX IF NOT EXISTS idx_api_keys_merchant ON public.api_keys(merchant_id);

-- 3. Enable Row Level Security (RLS)
ALTER TABLE public.merchants ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.api_keys ENABLE ROW LEVEL SECURITY;

-- 4. RLS Policies: Merchants can only see & manage their own data
CREATE POLICY "Merchants can view their own profile"
    ON public.merchants FOR SELECT
    USING (auth.uid() = user_id);

CREATE POLICY "Merchants can update their own profile"
    ON public.merchants FOR UPDATE
    USING (auth.uid() = user_id);

CREATE POLICY "Merchants can view their own API keys"
    ON public.api_keys FOR SELECT
    USING (
        merchant_id IN (SELECT id FROM public.merchants WHERE user_id = auth.uid())
    );

CREATE POLICY "Merchants can insert their own API keys"
    ON public.api_keys FOR INSERT
    WITH CHECK (
        merchant_id IN (SELECT id FROM public.merchants WHERE user_id = auth.uid())
    );

CREATE POLICY "Merchants can revoke/update their own API keys"
    ON public.api_keys FOR UPDATE
    USING (
        merchant_id IN (SELECT id FROM public.merchants WHERE user_id = auth.uid())
    );

CREATE POLICY "Merchants can delete their own API keys"
    ON public.api_keys FOR DELETE
    USING (
        merchant_id IN (SELECT id FROM public.merchants WHERE user_id = auth.uid())
    );

-- 5. Trigger to automatically create a merchant row when a new user signs up in Supabase
CREATE OR REPLACE FUNCTION public.handle_new_user()
RETURNS TRIGGER AS $$
BEGIN
    INSERT INTO public.merchants (user_id, email, organization_name)
    VALUES (
        NEW.id,
        NEW.email,
        COALESCE(NEW.raw_user_meta_data->>'organization_name', 'My Workspace')
    );
    RETURN NEW;
END;
$$ LANGUAGE plpgsql SECURITY DEFINER;

DROP TRIGGER IF EXISTS on_auth_user_created ON auth.users;
CREATE TRIGGER on_auth_user_created
    AFTER INSERT ON auth.users
    FOR EACH ROW EXECUTE FUNCTION public.handle_new_user();

-- 6. RPC Function for Public 1-Click Sandbox Key Generation (No Signup Required)
CREATE OR REPLACE FUNCTION public.create_guest_sandbox_key(
    p_name TEXT DEFAULT 'Evaluator Sandbox Key',
    p_key_prefix TEXT DEFAULT 'esa_test_demo_',
    p_key_hash TEXT DEFAULT ''
)
RETURNS JSONB AS $$
DECLARE
    v_merchant_id UUID;
    v_key_id UUID;
BEGIN
    -- Check or create demo merchant anchor
    SELECT id INTO v_merchant_id FROM public.merchants WHERE email = 'sandbox@esapay.internal';
    IF v_merchant_id IS NULL THEN
        INSERT INTO public.merchants (id, email, organization_name, tier)
        VALUES ('00000000-0000-0000-0000-000000000001', 'sandbox@esapay.internal', 'ESAPay Public Sandbox', 'developer')
        RETURNING id INTO v_merchant_id;
    END IF;

    -- Insert active demo key
    INSERT INTO public.api_keys (merchant_id, name, key_prefix, key_hash, environment, is_active)
    VALUES (v_merchant_id, p_name, p_key_prefix, p_key_hash, 'sandbox', true)
    RETURNING id INTO v_key_id;

    RETURN jsonb_build_object(
        'key_id', v_key_id,
        'prefix', p_key_prefix,
        'environment', 'sandbox',
        'status', 'active'
    );
END;
$$ LANGUAGE plpgsql SECURITY DEFINER;

-- ==============================================================================
-- 7. Bring Your Own Key (BYOK) - Merchant Provider Credentials (Rules 12, 13 & 59)
-- ==============================================================================
-- Stores encrypted references to customer-owned provider keys (OpenAI, Anthropic, Razorpay, etc.)
-- Enables strict tenant-level quota isolation so one tenant never exhausts another tenant's upstream quota.
CREATE TABLE IF NOT EXISTS public.merchant_credentials (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    merchant_id UUID REFERENCES public.merchants(id) ON DELETE CASCADE,
    provider TEXT NOT NULL CHECK (provider IN ('ollama', 'openai', 'anthropic', 'huggingface', 'razorpay', 'stripe', 'custom')),
    credential_type TEXT NOT NULL DEFAULT 'api_key', -- 'api_key', 'webhook_secret', 'endpoint_url'
    encrypted_secret TEXT NOT NULL,                  -- Encrypted credential or secret reference (never plaintext)
    key_prefix TEXT NOT NULL,                        -- First 8 characters for display (e.g. sk-proj-...)
    is_active BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_merchant_credentials_merchant ON public.merchant_credentials(merchant_id);

-- Enable RLS for BYOK credentials
ALTER TABLE public.merchant_credentials ENABLE ROW LEVEL SECURITY;

CREATE POLICY "Merchants can manage their own provider credentials"
    ON public.merchant_credentials FOR ALL
    USING (
        merchant_id IN (SELECT id FROM public.merchants WHERE user_id = auth.uid())
    );

