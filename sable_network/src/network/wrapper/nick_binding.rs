use crate::prelude::*;

/// A wrapper around a [`state::NickBinding`]
pub struct NickBinding<'a> {
    network: &'a Network,
    data: &'a state::NickBinding,
}

impl NickBinding<'_> {
    /// Return this object's ID
    pub fn nick(&self) -> Nickname {
        self.data.nick
    }

    pub fn user(&self) -> LookupResult<wrapper::User<'_>> {
        self.network.user(self.data.user)
    }

    pub fn timestamp(&self) -> i64 {
        self.data.timestamp
    }

    pub fn created(&self) -> EventId {
        self.data.created
    }
}

impl<'a> super::ObjectWrapper<'a> for NickBinding<'a> {
    type Underlying = state::NickBinding;

    fn wrap(network: &'a Network, data: &'a state::NickBinding) -> Self {
        Self { network, data }
    }

    fn raw(&self) -> &'a Self::Underlying {
        self.data
    }
}
