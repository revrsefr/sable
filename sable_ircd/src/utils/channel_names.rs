use crate::capability::ClientCapability;
use crate::errors::*;
use crate::messages::numeric;
use crate::messages::*;
use crate::*;
use sable_network::prelude::*;

use std::fmt::Write;

/// Returns a NAMES reply (or the implicit one after joining a channel)
pub fn send_channel_names(
    server: &ClientServer,
    to: impl MessageSink,
    to_user: &wrapper::User,
    channel: &wrapper::Channel,
) -> HandleResult {
    let mut lines = Vec::new();
    let mut current_line = String::new();
    const CONTENT_LEN: usize = 300;

    let pub_or_secret = if channel.mode().has_mode(ChannelModeFlag::Secret) {
        '@'
    } else {
        '='
    };

    let user_is_on_chan = to_user.is_in_channel(channel.id()).is_some();

    let with_userhost = to.capabilities().has(ClientCapability::UserhostInNames);
    let with_multiprefix = to.capabilities().has(ClientCapability::MultiPrefix);

    for member in channel.members() {
        if !user_is_on_chan
            && server
                .policy()
                .can_see_user_on_channel(to_user, &member)
                .is_err()
        {
            continue;
        }

        let p = if with_multiprefix {
            member.permissions().to_prefixes()
        } else {
            member
                .permissions()
                .to_highest_prefix()
                .as_ref()
                .map(char::to_string)
                .unwrap_or_else(|| "".to_string())
        };
        let user = member.user()?;
        let n = if with_userhost {
            format!("{}!{}@{}", user.nick(), user.user(), user.visible_host())
        } else {
            format!("{}", user.nick())
        };
        if current_line.len() + n.len() + 2 > CONTENT_LEN {
            lines.push(current_line);
            current_line = String::new();
        }
        current_line.write_fmt(format_args!("{p}{n} "))?;
    }
    current_line.pop(); // Remove trailing space
    lines.push(current_line);

    for line in lines {
        to.send(
            numeric::NamesReply::new(pub_or_secret, channel, &line).format_for(server, to_user),
        );
    }
    to.send(numeric::EndOfNames::new(channel.name().value()).format_for(server, to_user));
    Ok(())
}
