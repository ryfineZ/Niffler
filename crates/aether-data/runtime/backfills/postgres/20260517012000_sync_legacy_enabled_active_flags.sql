DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = 'public'
          AND table_name = 'providers'
          AND column_name = 'enabled'
    ) THEN
        UPDATE public.providers
        SET enabled = is_active
        WHERE is_active IS NOT NULL
          AND enabled IS DISTINCT FROM is_active;
    END IF;

    IF EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = 'public'
          AND table_name = 'provider_endpoints'
          AND column_name = 'enabled'
    ) THEN
        UPDATE public.provider_endpoints
        SET enabled = is_active
        WHERE is_active IS NOT NULL
          AND enabled IS DISTINCT FROM is_active;
    END IF;

    IF EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = 'public'
          AND table_name = 'models'
          AND column_name = 'enabled'
    ) THEN
        UPDATE public.models
        SET enabled = is_active
        WHERE is_active IS NOT NULL
          AND enabled IS DISTINCT FROM is_active;
    END IF;
END $$;
