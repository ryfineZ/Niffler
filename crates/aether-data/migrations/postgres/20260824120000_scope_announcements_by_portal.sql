ALTER TABLE public.announcements
    ADD COLUMN IF NOT EXISTS portal_id character varying(32);

UPDATE public.announcements
SET portal_id = 'default'
WHERE portal_id IS NULL OR BTRIM(portal_id) = '';

ALTER TABLE public.announcements
    ALTER COLUMN portal_id SET DEFAULT 'default',
    ALTER COLUMN portal_id SET NOT NULL;

CREATE INDEX IF NOT EXISTS announcements_portal_active_created_idx
    ON public.announcements (portal_id, is_active, created_at);
