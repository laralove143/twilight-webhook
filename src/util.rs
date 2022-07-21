use thiserror::Error;
use twilight_cache_inmemory::model::CachedMember;
use twilight_http::{request::channel::webhook::ExecuteWebhook, Client};
use twilight_model::{
    channel::Webhook,
    guild::{Member, PartialMember},
    id::{
        marker::{ChannelMarker, GuildMarker, UserMarker, WebhookMarker},
        Id,
    },
    user::User,
    util::ImageHash,
};

/// The errors that can be returned by utility methods
#[derive(Error, Debug)]
pub enum Error {
    /// The given webhook to make a [`MinimalWebhook`] from doesn't contain a
    /// token
    #[error("The given webhook to make a `MinimalWebhook` from doesn't contain a token")]
    NoToken,
    /// An error was returned by Twilight's HTTP client
    #[error("An error was returned by Twilight's HTTP client: {0}")]
    Http(#[from] twilight_http::error::Error),
    /// An error was returned by Twilight while validating the webhook
    #[error("An error was returned by Twilight while validating: {0}")]
    Validation(#[from] twilight_validate::message::MessageValidationError),
}

/// A struct with only the required information to execute webhooks
///
/// Implements `TryFrom<&Webhook>` for convenience, which might return
/// [`Error::NoToken`]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MinimalWebhook<'t> {
    /// The webhook's ID
    id: Id<WebhookMarker>,
    /// The webhook's token
    token: &'t str,
}

impl<'t> MinimalWebhook<'t> {
    /// Make a `MinimalWebhook` from a webhook ID and token, you may need this
    /// if you don't have a `Webhook`
    #[must_use]
    pub const fn new(id: Id<WebhookMarker>, token: &'t str) -> Self {
        Self { id, token }
    }
}

impl<'t> TryFrom<&'t Webhook> for MinimalWebhook<'t> {
    type Error = Error;

    fn try_from(webhook: &'t Webhook) -> Result<Self, Self::Error> {
        Ok(Self {
            id: webhook.id,
            token: webhook.token.as_ref().ok_or(Error::NoToken)?,
        })
    }
}

/// A struct with only the required information to execute webhooks as
/// members/users
///
/// Implements `From<&Member>` and `From<&User>` for convenience, if you only
/// have a `PartialMember` or `CachedMember`, use
/// [`MinimalMember::from_partial_member`] or [`MinimalMember::
/// from_cached_member`] to make sure you're falling back to the user
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MinimalMember<'u> {
    /// The member's nick or username
    name: &'u str,
    /// The CDN endpoint of the member's guild or user avatar, if the member has
    /// one
    avatar_url: Option<String>,
}

impl<'u> MinimalMember<'u> {
    /// Make a `MinimalMember` from a username or nick and avatar, you should
    /// only use this if the other methods or implementations don't work for
    /// you
    ///
    /// The user ID is only required if the user or member has an avatar
    ///
    /// # Warning for `guild_id`
    /// Pass the `guild_id` only if the avatar is a guild avatar, passing it
    /// with a user avatar will result in an invalid avatar URL
    #[must_use]
    pub fn new(
        name: &'u str,
        avatar: Option<(ImageHash, Id<UserMarker>)>,
        guild_id: Option<Id<GuildMarker>>,
    ) -> Self {
        Self {
            name,
            avatar_url: avatar.map(|(hash, user_id)| {
                guild_id.map_or_else(
                    || user_avatar_url(hash, user_id),
                    |id| member_avatar_url(hash, user_id, id),
                )
            }),
        }
    }

    /// Tries to use the member's nickname and avatar, falling back to the given
    /// user's name and avatar
    ///
    /// Uses a separate user parameter to make sure a user is passed
    ///
    /// The `guild_id` is required to use the member's avatar, if `None` is
    /// passed, only the user's avatar will be used
    #[must_use]
    pub fn from_partial_member(
        member: &'u PartialMember,
        guild_id: Option<Id<GuildMarker>>,
        user: &'u User,
    ) -> Self {
        Self {
            name: member.nick.as_ref().unwrap_or(&user.name),
            avatar_url: member
                .avatar
                .zip(guild_id)
                .map(|(hash, id)| member_avatar_url(hash, user.id, id))
                .or_else(|| user.avatar.map(|hash| user_avatar_url(hash, user.id))),
        }
    }

    /// Tries to use the member's nickname and avatar, falling back to the given
    /// user's name and avatar
    ///
    /// Uses a separate user parameter to make sure a user is passed
    #[must_use]
    pub fn from_cached_member(member: &'u CachedMember, user: &'u User) -> Self {
        Self {
            name: member.nick().unwrap_or(&user.name),
            avatar_url: member
                .avatar()
                .map(|hash| member_avatar_url(hash, user.id, member.guild_id()))
                .or_else(|| user.avatar.map(|hash| user_avatar_url(hash, user.id))),
        }
    }
}

impl<'u> From<&'u Member> for MinimalMember<'u> {
    fn from(member: &'u Member) -> Self {
        Self {
            name: member.nick.as_ref().unwrap_or(&member.user.name),
            avatar_url: member.avatar.map_or_else(
                || {
                    member
                        .user
                        .avatar
                        .map(|hash| user_avatar_url(hash, member.user.id))
                },
                |hash| Some(member_avatar_url(hash, member.user.id, member.guild_id)),
            ),
        }
    }
}

impl<'u> From<&'u User> for MinimalMember<'u> {
    fn from(user: &'u User) -> Self {
        Self {
            name: &user.name,
            avatar_url: user.avatar.map(|hash| user_avatar_url(hash, user.id)),
        }
    }
}

impl<'t> MinimalWebhook<'t> {
    /// Execute a webhook with the member's avatar and nick
    ///
    /// The `http` parameter is used to return its methods, it doesn't make any
    /// requests
    ///
    /// # Warning for thread channels
    /// You should call this on the parent channel's webhook if the channel is a
    /// thread, and pass the thread's channel ID you want to execute this
    /// webhook on
    ///
    /// # Errors
    /// Returns [`Error::Validation`] when the member's username/nick is invalid
    pub fn execute_as_member<'a>(
        &'a self,
        http: &'a Client,
        thread: Option<Id<ChannelMarker>>,
        member: &'a MinimalMember,
    ) -> Result<ExecuteWebhook<'a>, Error> {
        let mut exec = http
            .execute_webhook(self.id, self.token)
            .username(member.name)?;

        if let Some(id) = thread {
            exec = exec.thread_id(id);
        }

        if let Some(url) = &member.avatar_url {
            exec = exec.avatar_url(url);
        };

        Ok(exec)
    }
}

/// Returns the CDN endpoint for a user's avatar
fn user_avatar_url(hash: ImageHash, user_id: Id<UserMarker>) -> String {
    format!("https://cdn.discordapp.com/avatars/{user_id}/{hash}.png")
}

/// Returns the CDN endpoint for a member's avatar
fn member_avatar_url(
    hash: ImageHash,
    user_id: Id<UserMarker>,
    guild_id: Id<GuildMarker>,
) -> String {
    format!("https://cdn.discordapp.com/guilds/{guild_id}/users/{user_id}/avatars/{hash}.png",)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use twilight_cache_inmemory::InMemoryCache;
    use twilight_http::{
        client::ClientBuilder,
        request::{Request, TryIntoRequest},
    };
    use twilight_model::{
        gateway::payload::incoming::MemberAdd,
        guild::{Member, PartialMember},
        id::Id,
        user::User,
        util::{ImageHash, Timestamp},
    };

    use crate::util::{MinimalMember, MinimalWebhook};

    #[allow(clippy::unwrap_used)]
    fn user() -> User {
        User {
            name: "username".to_owned(),
            avatar: Some(ImageHash::from_str("a_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap()),
            id: Id::new(2),
            discriminator: 0,
            bot: false,
            accent_color: None,
            banner: None,
            email: None,
            flags: None,
            locale: None,
            mfa_enabled: None,
            premium_type: None,
            public_flags: None,
            system: None,
            verified: None,
        }
    }

    #[allow(clippy::unwrap_used)]
    fn member() -> Member {
        Member {
            nick: Some("nick".to_owned()),
            avatar: Some(ImageHash::from_str("a_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb").unwrap()),
            guild_id: Id::new(1),
            joined_at: Timestamp::from_secs(0).unwrap(),
            deaf: false,
            mute: false,
            pending: false,
            communication_disabled_until: None,
            premium_since: None,
            roles: vec![],
            user: user(),
        }
    }

    #[allow(clippy::unwrap_used)]
    fn member_user() -> Member {
        Member {
            nick: None,
            avatar: None,
            guild_id: Id::new(1),
            joined_at: Timestamp::from_secs(0).unwrap(),
            deaf: false,
            mute: false,
            pending: false,
            communication_disabled_until: None,
            premium_since: None,
            roles: vec![],
            user: user(),
        }
    }

    #[allow(clippy::unwrap_used)]
    fn partial_member() -> PartialMember {
        PartialMember {
            nick: Some("nick".to_owned()),
            avatar: Some(ImageHash::from_str("a_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb").unwrap()),
            joined_at: Timestamp::from_secs(0).unwrap(),
            deaf: false,
            mute: false,
            communication_disabled_until: None,
            premium_since: None,
            user: None,
            permissions: None,
            roles: vec![],
        }
    }

    #[allow(clippy::unwrap_used)]
    fn minimal_member<'u>() -> MinimalMember<'u> {
        MinimalMember {
            name: "nick",
            avatar_url: Some(
                "https://cdn.discordapp.com/guilds/1/users/2/avatars/\
                a_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.png"
                    .to_owned(),
            ),
        }
    }

    #[allow(clippy::unwrap_used)]
    fn minimal_member_user<'u>() -> MinimalMember<'u> {
        MinimalMember {
            name: "username",
            avatar_url: Some(
                "https://cdn.discordapp.com/avatars/2/a_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png"
                    .to_owned(),
            ),
        }
    }

    #[allow(clippy::unwrap_used)]
    fn cmp_requests(a: &Request, b: &Request) {
        assert_eq!(a.body(), b.body());
        assert_eq!(a.form().is_some(), b.form().is_some());
        assert_eq!(a.headers(), b.headers());
        assert_eq!(a.method(), b.method());
        assert_eq!(a.path(), b.path());
        assert_eq!(a.ratelimit_path(), b.ratelimit_path());
        assert_eq!(a.use_authorization_token(), a.use_authorization_token());
    }

    #[test]
    #[allow(clippy::unwrap_used)]
    fn new_minimal_member() {
        let mut minimal_member = minimal_member();
        assert_eq!(
            MinimalMember::new(
                "nick",
                Some((
                    ImageHash::from_str("a_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb").unwrap(),
                    Id::new(2)
                )),
                Some(Id::new(1))
            ),
            minimal_member
        );

        minimal_member.avatar_url = Some(
            "https://cdn.discordapp.com/avatars/2/a_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png"
                .to_owned(),
        );
        assert_eq!(
            MinimalMember::new(
                "nick",
                Some((
                    ImageHash::from_str("a_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap(),
                    Id::new(2)
                )),
                None
            ),
            minimal_member
        );

        minimal_member.name = "username";
        assert_eq!(
            MinimalMember::new(
                "username",
                Some((
                    ImageHash::from_str("a_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap(),
                    Id::new(2)
                )),
                None
            ),
            minimal_member
        );
    }

    #[test]
    fn from_member() {
        let mut member = member();
        let mut minimal_member = minimal_member();
        assert_eq!(MinimalMember::from(&member), minimal_member);

        member.avatar = None;
        minimal_member.avatar_url = Some(
            "https://cdn.discordapp.com/avatars/2/a_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png"
                .to_owned(),
        );
        assert_eq!(MinimalMember::from(&member), minimal_member);

        member.nick = None;
        minimal_member.name = "username";
        assert_eq!(MinimalMember::from(&member), minimal_member);
    }

    #[test]
    fn from_member_user() {
        let mut member_user = member_user();
        let mut minimal_member = minimal_member_user();
        assert_eq!(MinimalMember::from(&member_user), minimal_member_user());

        member_user.user.avatar = None;
        minimal_member.avatar_url = None;
        assert_eq!(MinimalMember::from(&member_user), minimal_member);
    }

    #[test]
    fn from_partial_member() {
        let mut member = partial_member();
        let mut minimal_member = minimal_member();
        let guild_id = Some(Id::new(1));
        let user = user();
        assert_eq!(
            MinimalMember::from_partial_member(&member, guild_id, &user),
            minimal_member
        );

        member.avatar = None;
        minimal_member.avatar_url = Some(
            "https://cdn.discordapp.com/avatars/2/a_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png"
                .to_owned(),
        );
        assert_eq!(
            MinimalMember::from_partial_member(&member, guild_id, &user),
            minimal_member
        );

        member.nick = None;
        minimal_member.name = "username";
        assert_eq!(
            MinimalMember::from_partial_member(&member, guild_id, &user),
            minimal_member
        );
    }

    #[test]
    #[allow(clippy::unwrap_used)]
    fn from_cached_member() {
        let cache = InMemoryCache::new();
        let mut member = member();
        let mut minimal_member = minimal_member();

        cache.update(&MemberAdd(member.clone()));
        assert_eq!(
            MinimalMember::from_cached_member(
                cache
                    .member(member.guild_id, member.user.id)
                    .unwrap()
                    .value(),
                &member.user
            ),
            minimal_member
        );

        member.avatar = None;
        minimal_member.avatar_url = Some(
            "https://cdn.discordapp.com/avatars/2/a_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png"
                .to_owned(),
        );
        cache.update(&MemberAdd(member.clone()));
        assert_eq!(MinimalMember::from(&member), minimal_member);

        member.nick = None;
        minimal_member.name = "username";
        cache.update(&MemberAdd(member.clone()));
        assert_eq!(MinimalMember::from(&member), minimal_member);
    }

    #[test]
    fn from_cached_member_user() {
        let cache = InMemoryCache::new();
        let mut member_user = member_user();
        let mut minimal_member = minimal_member_user();

        cache.update(&MemberAdd(member_user.clone()));
        assert_eq!(MinimalMember::from(&member_user), minimal_member_user());

        member_user.user.avatar = None;
        minimal_member.avatar_url = None;
        cache.update(&MemberAdd(member_user.clone()));
        assert_eq!(MinimalMember::from(&member_user), minimal_member);
    }

    #[test]
    fn from_user() {
        let mut user = user();
        let mut minimal_member_user = minimal_member_user();
        assert_eq!(MinimalMember::from(&user), minimal_member_user);

        user.avatar = None;
        minimal_member_user.avatar_url = None;
        assert_eq!(MinimalMember::from(&user), minimal_member_user);
    }

    #[test]
    #[allow(clippy::unwrap_used)]
    fn execute_as_member() {
        let webhook = MinimalWebhook {
            id: Id::new(1),
            token: "a",
        };
        let http = ClientBuilder::new().build();

        let request_a = webhook
            .execute_as_member(&http, None, &minimal_member())
            .unwrap()
            .try_into_request()
            .unwrap();
        let request_b = http
            .execute_webhook(Id::new(1), "a")
            .username("nick")
            .unwrap()
            .avatar_url(
                "https://cdn.discordapp.com/guilds/1/users/2/avatars/\
                a_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.png",
            )
            .try_into_request()
            .unwrap();

        cmp_requests(&request_a, &request_b);
    }

    #[test]
    #[allow(clippy::unwrap_used)]
    fn execute_as_member_thread() {
        let thread_id = Id::new(1);

        let webhook = MinimalWebhook {
            id: Id::new(1),
            token: "a",
        };
        let http = ClientBuilder::new().build();

        let request_a = webhook
            .execute_as_member(&http, Some(thread_id), &minimal_member())
            .unwrap()
            .try_into_request()
            .unwrap();
        let request_b = http
            .execute_webhook(Id::new(1), "a")
            .username("nick")
            .unwrap()
            .avatar_url(
                "https://cdn.discordapp.com/guilds/1/users/2/avatars/\
                a_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.png",
            )
            .thread_id(thread_id)
            .try_into_request()
            .unwrap();

        cmp_requests(&request_a, &request_b);
    }

    #[test]
    #[allow(clippy::unwrap_used)]
    fn execute_as_member_user() {
        let webhook = MinimalWebhook {
            id: Id::new(1),
            token: "a",
        };
        let http = ClientBuilder::new().build();

        let request_a = webhook
            .execute_as_member(&http, None, &minimal_member_user())
            .unwrap()
            .try_into_request()
            .unwrap();
        let request_b = http
            .execute_webhook(Id::new(1), "a")
            .username("username")
            .unwrap()
            .avatar_url(
                "https://cdn.discordapp.com/avatars/2/a_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png",
            )
            .try_into_request()
            .unwrap();

        cmp_requests(&request_a, &request_b);
    }
}
