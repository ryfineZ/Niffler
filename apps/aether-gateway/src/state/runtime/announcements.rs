use crate::{AppState, GatewayError};

impl AppState {
    pub(crate) async fn list_announcements(
        &self,
        portal_id: &str,
        query: &aether_data::repository::announcements::AnnouncementListQuery,
    ) -> Result<aether_data::repository::announcements::StoredAnnouncementPage, GatewayError> {
        self.data
            .list_announcements(portal_id, query)
            .await
            .map_err(|err| GatewayError::Internal(err.to_string()))
    }

    pub(crate) async fn find_announcement_by_id(
        &self,
        portal_id: &str,
        announcement_id: &str,
    ) -> Result<Option<aether_data::repository::announcements::StoredAnnouncement>, GatewayError>
    {
        self.data
            .find_announcement_by_id(portal_id, announcement_id)
            .await
            .map_err(|err| GatewayError::Internal(err.to_string()))
    }

    pub(crate) async fn count_unread_active_announcements(
        &self,
        portal_id: &str,
        user_id: &str,
        now_unix_secs: u64,
    ) -> Result<u64, GatewayError> {
        self.data
            .count_unread_active_announcements(portal_id, user_id, now_unix_secs)
            .await
            .map_err(|err| GatewayError::Internal(err.to_string()))
    }

    pub(crate) async fn list_required_unread_active_announcements(
        &self,
        portal_id: &str,
        user_id: &str,
        now_unix_secs: u64,
        limit: usize,
    ) -> Result<Vec<aether_data::repository::announcements::StoredAnnouncement>, GatewayError> {
        self.data
            .list_required_unread_active_announcements(portal_id, user_id, now_unix_secs, limit)
            .await
            .map_err(|err| GatewayError::Internal(err.to_string()))
    }

    pub(crate) async fn create_announcement(
        &self,
        portal_id: &str,
        record: aether_data::repository::announcements::CreateAnnouncementRecord,
    ) -> Result<Option<aether_data::repository::announcements::StoredAnnouncement>, GatewayError>
    {
        self.data
            .create_announcement(portal_id, record)
            .await
            .map_err(|err| GatewayError::Internal(err.to_string()))
    }

    pub(crate) async fn update_announcement(
        &self,
        portal_id: &str,
        record: aether_data::repository::announcements::UpdateAnnouncementRecord,
    ) -> Result<Option<aether_data::repository::announcements::StoredAnnouncement>, GatewayError>
    {
        self.data
            .update_announcement(portal_id, record)
            .await
            .map_err(|err| GatewayError::Internal(err.to_string()))
    }

    pub(crate) async fn delete_announcement(
        &self,
        portal_id: &str,
        announcement_id: &str,
    ) -> Result<bool, GatewayError> {
        self.data
            .delete_announcement(portal_id, announcement_id)
            .await
            .map_err(|err| GatewayError::Internal(err.to_string()))
    }

    pub(crate) async fn mark_announcement_as_read(
        &self,
        portal_id: &str,
        user_id: &str,
        announcement_id: &str,
        read_at_unix_secs: u64,
    ) -> Result<bool, GatewayError> {
        self.data
            .mark_announcement_as_read(portal_id, user_id, announcement_id, read_at_unix_secs)
            .await
            .map_err(|err| GatewayError::Internal(err.to_string()))
    }
}
