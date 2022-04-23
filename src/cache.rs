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
    #[must_use]
    pub fn new() -> Self {
        Self(DashMap::new())
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

    /// replaces the webhooks from the cache with the ones returned by the http
    /// client
    ///
    /// you should run this on `WebhooksUpdate`, `GuildCreate` and
    /// `ChannelCreate` events
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
    pub async fn update(&self, http: &Client, channel_id: Id<ChannelMarker>) -> Result<(), Error> {
        self.0.remove(&channel_id);

        http.channel_webhooks(channel_id)
            .exec()
            .await?
            .models()
            .await?
            .into_iter()
            .filter(|w| w.token.is_some())
            .for_each(|w| {
                self.0.insert(channel_id, w);
            });

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
}

#[cfg(test)]
mod tests {
    use twilight_http::Client;
    use twilight_model::{
        channel::{Webhook, WebhookType},
        id::{marker::ChannelMarker, Id},
    };

    use crate::cache::Cache;

    const CHANNEL_ID: Id<ChannelMarker> = Id::new(903_367_566_175_653_969);
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

    #[tokio::test]
    #[allow(clippy::unwrap_used)]
    async fn update() {
        let cache = Cache::new();
        let http = Client::new(env!("TEST_BOT_TOKEN").to_owned());

        assert!(cache.get(CHANNEL_ID).is_none());
        cache.update(&http, CHANNEL_ID).await.unwrap();
        assert!(cache.get(CHANNEL_ID).is_none());

        http.create_webhook(CHANNEL_ID, "test")
            .exec()
            .await
            .unwrap();
        cache.update(&http, CHANNEL_ID).await.unwrap();
        assert!(cache.get(CHANNEL_ID).is_some());
    }

    #[tokio::test]
    #[allow(clippy::unwrap_used)]
    async fn create() {
        let cache = Cache::new();
        let http = Client::new(env!("TEST_BOT_TOKEN").to_owned());

        assert!(cache.get(CHANNEL_ID).is_none());

        cache
            .create(http.create_webhook(CHANNEL_ID, "test"))
            .await
            .unwrap();

        assert!(cache.get(CHANNEL_ID).is_some());
    }

    #[test]
    fn get() {
        let cache = Cache::new();
        cache.0.insert(Id::new(1), WEBHOOK);

        assert!(cache.get(Id::new(2)).is_none());

        assert_eq!(cache.get(Id::new(1)).as_deref(), Some(&WEBHOOK));
    }
}
