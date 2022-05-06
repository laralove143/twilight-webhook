use dashmap::{mapref::one::Ref, DashMap};
use thiserror::Error;
use twilight_http::{request::channel::webhook::CreateWebhook, Client};
use twilight_model::{
    channel::Webhook,
    id::{marker::ChannelMarker, Id},
};

#[derive(Error, Debug)]
/// an error occurred when trying to update the cache
pub enum Error {
    /// an error was returned by twilight's http client
    #[error("an error was returned by twilight's http client: {0}")]
    Http(#[from] twilight_http::error::Error),
    /// an error was returned by twilight's http client while trying to
    /// deserialize the response
    #[error(
        "an error was returned by twilight's http client while trying to deserialize the \
         response: {0}"
    )]
    Deserialize(#[from] twilight_http::response::DeserializeBodyError),
}

/// cache to hold webhooks, keyed by channel ids for general usage
pub struct Cache(DashMap<Id<ChannelMarker>, Webhook>);

impl Default for Cache {
    fn default() -> Self {
        Self::new()
    }
}

impl Cache {
    /// creates a new webhook cache
    ///
    /// # invalidation warning
    /// you should run [`Self::validate`] on `WebhookUpdate` events to make sure
    /// manually deleted webhooks are removed from the cache, otherwise
    /// executing a cached webhook will return "Unknown Webhook" errors
    #[must_use]
    pub fn new() -> Self {
        Self(DashMap::new())
    }

    /// convenience function to get from the cache, requesting it from the api
    /// if it doesn't exist, creating it if it's also not returned
    ///
    /// # Errors
    /// returns an [`Error::Http`] or [`Error::Deserialize`] if the webhook
    /// isn't in the cache
    ///
    /// # Panics
    /// if the webhook that was just inserted to the cache doesn't exist somehow
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
                http.create_webhook(channel_id, name)
                    .exec()
                    .await?
                    .model()
                    .await?
            };
            self.0.insert(channel_id, webhook);
            Ok(self.get(channel_id).unwrap())
        }
    }

    /// creates the passed webhook and caches it, it takes a `CreateWebhook`
    /// instead of a `Webhook` to reduce boilerplate and avoid clones
    ///
    /// # race condition warning
    /// webhooks created without using this function will eventually be cached
    /// by the [`Self::update`] method, but may not be immediately available to
    /// access
    ///
    /// # Errors
    /// returns an [`Error::Http`] or [`Error::Deserialize`]
    pub async fn create<'a>(&self, create_webhook: CreateWebhook<'a>) -> Result<(), Error> {
        let webhook = create_webhook.exec().await?.model().await?;
        self.0.insert(webhook.channel_id, webhook);

        Ok(())
    }

    /// returns the webhook for the given channel id, if it exists
    #[must_use]
    pub fn get(
        &self,
        channel_id: Id<ChannelMarker>,
    ) -> Option<Ref<'_, Id<ChannelMarker>, Webhook>> {
        self.0.get(&channel_id)
    }

    /// validates the cache by retrieving the webhooks from the api, this is
    /// because discord doesn't send info about updated webhooks in the events,
    /// you should run this on `WebhooksUpdate` events
    ///
    /// # Errors
    /// returns [`Error::Http`] or [`Error::Deserialize`]
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

    /// replaces the webhooks from the cache with the ones returned by the http
    /// client
    ///
    /// the http client is used because `WebhooksUpdate` events don't contain
    /// webhook information
    ///
    /// # possible overhead
    /// try not to call this on channels whose webhooks you won't use, as it
    /// makes an http request every time
    ///
    /// # Errors
    /// returns an [`Error::Http`] or [`Error::Deserialize`]
    #[deprecated(note = "use `get_or_create` to only request webhooks you actually need")]
    pub async fn update(&self, http: &Client, channel_id: Id<ChannelMarker>) -> Result<(), Error> {
        self.0.remove(&channel_id);

        http.channel_webhooks(channel_id)
            .exec()
            .await?
            .models()
            .await?
            .into_iter()
            .find(|w| w.token.is_some())
            .and_then(|w| self.0.insert(channel_id, w));

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use twilight_model::{
        channel::{Webhook, WebhookType},
        id::Id,
    };

    use crate::cache::Cache;

    const WEBHOOK: Webhook = Webhook {
        id: Id::new(1),
        channel_id: Id::new(1),
        kind: WebhookType::Application,
        application_id: None,
        avatar: None,
        guild_id: None,
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
}
