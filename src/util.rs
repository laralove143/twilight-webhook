use thiserror::Error;
use twilight_http::{request::channel::webhook::ExecuteWebhook, Client};
use twilight_model::{
    channel::Webhook,
    guild::Member,
    id::{
        marker::{ChannelMarker, WebhookMarker},
        Id,
    },
    user::User,
};

/// the errors that can be returned by utility methods
#[derive(Error, Debug)]
pub enum Error {
    /// the given webhook to make a minimal webhook from contains no token
    #[error("the given webhook to make a minimal webhook from contains no token")]
    NoToken,
    /// the given partial member to make a minimal member from contains no nick
    /// or user
    #[error("the given partial member to make a minimal member from contains no nick or user")]
    NoNickOrUser,
    /// an error was returned by twilight's http client
    #[error("an error was returned by twilight's http client: {0}")]
    Http(#[from] twilight_http::error::Error),
}

/// a struct with only the required information to execute webhooks
///
/// this implements `TryFrom<Webhook>` for convenience, which might return
/// [`Error::NoToken`]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MinimalWebhook {
    /// the webhook's id, required when executing it
    id: Id<WebhookMarker>,
    /// the webhook's token, required when executing it
    token: String,
}

impl TryFrom<Webhook> for MinimalWebhook {
    type Error = Error;

    fn try_from(webhook: Webhook) -> Result<Self, Self::Error> {
        Ok(Self {
            id: webhook.id,
            token: webhook.token.ok_or(Error::NoToken)?,
        })
    }
}

/// a struct with only the required information to execute webhooks as
/// members/users
///
/// this implements `From<Member>` and `From<User>` for convenience, the
/// `From<Member>` implementation tries to use the member's guild nick and
/// avatar, falling back to the user's name and avatar
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MinimalMember {
    /// the member's nick or name
    name: String,
    /// the cdn endpoint of the member's guild or user avatar, if the member has
    /// one
    avatar_url: Option<String>,
}

impl From<Member> for MinimalMember {
    fn from(member: Member) -> Self {
        Self {
            name: member.nick.unwrap_or(member.user.name),
            avatar_url: member.avatar.map_or_else(
                || {
                    Some(format!(
                        "https://cdn.discordapp.com/avatars/{}/{}.png",
                        member.user.id, member.user.avatar?
                    ))
                },
                |hash| {
                    Some(format!(
                        "https://cdn.discordapp.com/guilds/{}/users/{}/avatars/{hash}.png",
                        member.guild_id, member.user.id
                    ))
                },
            ),
        }
    }
}

impl From<User> for MinimalMember {
    fn from(user: User) -> Self {
        Self {
            name: user.name,
            avatar_url: user
                .avatar
                .map(|hash| format!("https://cdn.discordapp.com/avatars/{}/{hash}.png", user.id)),
        }
    }
}

impl MinimalWebhook {
    /// send a webhook with the member's avatar and nick
    /// this takes the http client to return its methods, it doesn't make any
    /// requests
    ///
    /// # warning for thread channels
    /// you should call this on the parent channel's webhook if the channel is a
    /// thread, and pass the thread's channel id you want to send this webhook
    /// to
    pub fn execute_as_member<'a>(
        &'a self,
        http: &'a Client,
        thread: Option<Id<ChannelMarker>>,
        member: &'a MinimalMember,
    ) -> ExecuteWebhook<'a> {
        let mut exec = http
            .execute_webhook(self.id, &self.token)
            .username(&member.name);

        if let Some(id) = thread {
            exec = exec.thread_id(id);
        }

        if let Some(url) = &member.avatar_url {
            exec = exec.avatar_url(url);
        };

        exec
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use twilight_http::{
        client::ClientBuilder,
        request::{Request, TryIntoRequest},
    };
    use twilight_model::{datetime::Timestamp, guild::Member, id::Id, user::User, util::ImageHash};

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
    fn minimal_member() -> MinimalMember {
        MinimalMember {
            name: "nick".to_owned(),
            avatar_url: Some(
                "https://cdn.discordapp.com/guilds/1/users/2/avatars/\
                a_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.png"
                    .to_owned(),
            ),
        }
    }

    #[allow(clippy::unwrap_used)]
    fn minimal_member_user() -> MinimalMember {
        MinimalMember {
            name: "username".to_owned(),
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
    fn from_member() {
        let mut member = member();
        let mut minimal_member = minimal_member();
        assert_eq!(MinimalMember::from(member.clone()), minimal_member);

        member.avatar = None;
        minimal_member.avatar_url = Some(
            "https://cdn.discordapp.com/avatars/2/a_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png"
                .to_owned(),
        );
        assert_eq!(MinimalMember::from(member.clone()), minimal_member);

        member.nick = None;
        minimal_member.name = "username".to_owned();
        assert_eq!(MinimalMember::from(member), minimal_member);
    }

    #[test]
    fn from_member_user() {
        let mut member_user = member_user();
        let mut minimal_member = minimal_member_user();
        assert_eq!(
            MinimalMember::from(member_user.clone()),
            minimal_member_user()
        );

        member_user.user.avatar = None;
        minimal_member.avatar_url = None;
        assert_eq!(MinimalMember::from(member_user), minimal_member);
    }

    #[test]
    fn from_user() {
        let mut user = user();
        let mut minimal_member_user = minimal_member_user();
        assert_eq!(MinimalMember::from(user.clone()), minimal_member_user);

        user.avatar = None;
        minimal_member_user.avatar_url = None;
        assert_eq!(MinimalMember::from(user), minimal_member_user);
    }

    #[test]
    #[allow(clippy::unwrap_used)]
    fn execute_as_member() {
        let webhook = MinimalWebhook {
            id: Id::new(1),
            token: "a".to_owned(),
        };
        let http = ClientBuilder::new().build();

        let request_a = webhook
            .execute_as_member(&http, None, &minimal_member())
            .try_into_request()
            .unwrap();
        let request_b = http
            .execute_webhook(Id::new(1), "a")
            .username("nick")
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
    fn execute_as_member_user() {
        let webhook = MinimalWebhook {
            id: Id::new(1),
            token: "a".to_owned(),
        };
        let http = ClientBuilder::new().build();

        let request_a = webhook
            .execute_as_member(&http, None, &minimal_member_user())
            .try_into_request()
            .unwrap();
        let request_b = http
            .execute_webhook(Id::new(1), "a")
            .username("username")
            .avatar_url(
                "https://cdn.discordapp.com/avatars/2/a_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png",
            )
            .try_into_request()
            .unwrap();

        cmp_requests(&request_a, &request_b);
    }
}