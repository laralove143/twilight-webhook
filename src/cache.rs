use dashmap::{mapref::one::Ref, DashMap};
use thiserror::Error;
use twilight_http::{request::channel::webhook::CreateWebhook, Client};
use twilight_model::{
    channel::Webhook,
    gateway::event::Event,
    id::{marker::ChannelMarker, Id},
};

#[derive(Error, Debug)]
/// An error occurred when trying to update the cache
pub enum Error {
    /// An error was returned by Twilight's HTTP client while making the request
    #[error("An error was returned by Twilight's HTTP client: {0}")]
    Http(#[from] twilight_http::error::Error),
    /// An error was returned by Twilight's HTTP client while deserializing the
    /// response
    #[error(
        "An error was returned by Twilight's HTTP client while deserializing the response: {0}"
    )]
    Deserialize(#[from] twilight_http::response::DeserializeBodyError),
    /// An error was returned by Twilight while validating a request
    #[error("An error was returned by Twilight while validating a request: {0}")]
    Validation(#[from] twilight_validate::request::ValidationError),
}

/// Cache to hold webhooks, keyed by channel IDs for general usage
#[derive(Debug)]
pub struct Cache(DashMap<Id<ChannelMarker>, Webhook>);

impl Default for Cache {
    fn default() -> Self {
        Self::new()
    }
}

impl Cache {
    /// Creates a new webhook cache
    ///
    /// # Invalidation warning
    /// Make sure you receive `ChannelDelete` and `GuildDelete` events and call
    /// [`Cache::update`] method on them to remove inaccessible webhooks from
    /// the cache
    ///
    /// Also make sure you call [`Cache::validate`] method on `WebhookUpdate`
    /// events to remove manually deleted webhooks from the cache
    #[must_use]
    pub fn new() -> Self {
        Self(DashMap::new())
    }

    /// Convenience function to get from the cache, requesting it from the API
    /// if it doesn't exist, creating it if it's also not returned
    ///
    /// # Required permissions
    /// Make sure the bot has `MANAGE_WEBHOOKS` permission in the given channel
    ///
    /// # Errors
    /// Returns an [`Error::Http`] or [`Error::Deserialize`] if the webhook
    /// isn't in the cache
    ///
    /// # Panics
    /// If the webhook that was just inserted to the cache somehow doesn't exist
    #[allow(clippy::unwrap_used)]
    pub async fn get_infallible<'a>(
        &self,
        http: &Client,
        channel_id: Id<ChannelMarker>,
        name: &str,
    ) -> Result<Ref<'_, Id<ChannelMarker>, Webhook>, Error> {
        if let Some(webhook) = self.get(channel_id) {
            Ok(webhook)
        } else {
            let webhook = if let Some(webhook) = http
                .channel_webhooks(channel_id)
                .exec()
                .await?
                .models()
                .await?
                .into_iter()
                .find(|w| w.token.is_some())
            {
                webhook
            } else {
                http.create_webhook(channel_id, name)?
                    .exec()
                    .await?
                    .model()
                    .await?
            };
            self.0.insert(channel_id, webhook);
            Ok(self.get(channel_id).unwrap())
        }
    }

    /// Creates the passed webhook and caches it, it takes a `CreateWebhook`
    /// instead of a `Webhook` to reduce boilerplate and avoid clones
    ///
    /// # Errors
    /// Returns [`Error::Http`] or [`Error::Deserialize`]
    pub async fn create<'a>(&self, create_webhook: CreateWebhook<'a>) -> Result<(), Error> {
        let webhook = create_webhook.exec().await?.model().await?;
        self.0.insert(webhook.channel_id, webhook);

        Ok(())
    }

    /// Returns the webhook for the given `channel_id`, if it exists
    #[must_use]
    pub fn get(
        &self,
        channel_id: Id<ChannelMarker>,
    ) -> Option<Ref<'_, Id<ChannelMarker>, Webhook>> {
        self.0.get(&channel_id)
    }

    /// Validates the cache by retrieving the webhooks from the API
    ///
    /// Using the API is required because Discord doesn't send info about
    /// updated webhooks in the events
    ///
    /// # Invalidation warning
    /// You should run this on `WebhookUpdate` events to make sure deleted
    /// webhooks are removed from the cache, otherwise, executing a
    /// cached webhook will return `Unknown Webhook` errors
    ///
    /// # Required permissions
    /// If the given `channel_id` is in the cache, `MANAGE_WEBHOOKS` permission
    /// is required
    ///
    /// # Errors
    /// Returns [`Error::Http`] or [`Error::Deserialize`]
    pub async fn validate(
        &self,
        http: &Client,
        channel_id: Id<ChannelMarker>,
    ) -> Result<(), Error> {
        if !self.0.contains_key(&channel_id) {
            return Ok(());
        }

        if !http
            .channel_webhooks(channel_id)
            .exec()
            .await?
            .models()
            .await?
            .iter()
            .any(|webhook| webhook.token.is_some())
        {
            self.0.remove(&channel_id);
        }

        Ok(())
    }

    /// Removes the cached webhooks for the given event's channel or guild if
    /// the event is `ChannelDelete` or `GuildDelete`
    #[allow(clippy::wildcard_enum_match_arm)]
    pub fn update(&self, event: &Event) {
        match event {
            Event::ChannelDelete(channel) => {
                self.0.remove(&channel.id);
            }
            Event::GuildDelete(guild) => self
                .0
                .retain(|_, webhook| webhook.guild_id != Some(guild.id)),
            _ => (),
        };
    }
}

#[cfg(test)]
mod tests {
    use twilight_model::{
        channel::{Channel, ChannelType, Webhook, WebhookType},
        gateway::{
            event::Event,
            payload::incoming::{ChannelDelete, GuildDelete},
        },
        id::Id,
    };

    use crate::cache::Cache;

    const WEBHOOK: Webhook = Webhook {
        id: Id::new(1),
        channel_id: Id::new(1),
        kind: WebhookType::Application,
        application_id: None,
        avatar: None,
        guild_id: Some(Id::new(10)),
        name: None,
        source_channel: None,
        source_guild: None,
        token: None,
        url: None,
        user: None,
    };

    #[test]
    fn get() {
        let cache = Cache::new();
        cache.0.insert(Id::new(1), WEBHOOK);

        assert!(cache.get(Id::new(2)).is_none());

        assert_eq!(cache.get(Id::new(1)).as_deref(), Some(&WEBHOOK));
    }

    #[test]
    fn update() {
        let cache = Cache::new();
        cache.0.insert(Id::new(1), WEBHOOK);
        cache.0.insert(Id::new(2), WEBHOOK);

        cache.update(&Event::GuildDelete(GuildDelete {
            id: Id::new(11),
            unavailable: false,
        }));
        assert_eq!(cache.get(Id::new(1)).as_deref(), Some(&WEBHOOK));

        cache.update(&Event::GuildDelete(GuildDelete {
            id: Id::new(10),
            unavailable: false,
        }));
        assert!(cache.get(Id::new(1)).is_none());
        assert!(cache.get(Id::new(2)).is_none());

        cache.0.insert(Id::new(3), WEBHOOK);
        cache.update(&Event::ChannelDelete(Box::new(ChannelDelete(Channel {
            id: Id::new(3),
            guild_id: Some(Id::new(10)),
            kind: ChannelType::GuildText,
            application_id: None,
            bitrate: None,
            default_auto_archive_duration: None,
            icon: None,
            invitable: None,
            last_message_id: None,
            last_pin_timestamp: None,
            member: None,
            member_count: None,
            message_count: None,
            name: None,
            newly_created: None,
            nsfw: None,
            owner_id: None,
            parent_id: None,
            permission_overwrites: None,
            position: None,
            rate_limit_per_user: None,
            recipients: None,
            rtc_region: None,
            thread_metadata: None,
            topic: None,
            user_limit: None,
            video_quality_mode: None,
        }))));
        assert!(cache.get(Id::new(3)).is_none());
    }
}
