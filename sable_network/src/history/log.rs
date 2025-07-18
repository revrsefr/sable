use crate::prelude::*;

use serde::{ser::SerializeSeq, Deserialize, Serialize};
use serde_with::serde_as;
use std::collections::HashMap;

use parking_lot::{MappedRwLockReadGuard, RwLock, RwLockReadGuard, RwLockUpgradableReadGuard};

use concurrent_log::ConcurrentLog;

pub type LogEntryId = usize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryLogEntry {
    pub id: LogEntryId,
    pub timestamp: i64,
    pub source_event: EventId,
    pub details: NetworkStateChange,
}

type UserHistoryLog = ConcurrentLog<LogEntryId>;

struct UserLogMapConversion;

impl serde_with::SerializeAs<RwLock<HashMap<UserId, UserHistoryLog>>> for UserLogMapConversion {
    fn serialize_as<S>(
        source: &RwLock<HashMap<UserId, UserHistoryLog>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let lock = source.read_recursive();
        let mut seq = serializer.serialize_seq(Some(lock.len()))?;
        for pair in lock.iter() {
            seq.serialize_element(&pair)?;
        }
        seq.end()
    }
}

impl<'de> serde_with::DeserializeAs<'de, RwLock<HashMap<UserId, UserHistoryLog>>>
    for UserLogMapConversion
{
    fn deserialize_as<D>(
        deserializer: D,
    ) -> Result<RwLock<HashMap<UserId, UserHistoryLog>>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let vec = Vec::<(UserId, UserHistoryLog)>::deserialize(deserializer)?;
        let mut map = HashMap::new();
        for (k, v) in vec {
            map.insert(k, v);
        }
        Ok(RwLock::new(map))
    }
}

#[serde_as]
#[derive(Debug, Serialize, Deserialize)]
pub struct NetworkHistoryLog {
    pub(super) entries: ConcurrentLog<HistoryLogEntry>,
    #[serde_as(as = "UserLogMapConversion")]
    pub(super) user_logs: RwLock<HashMap<UserId, UserHistoryLog>>,
}

pub struct UserHistoryLogIterator<'a> {
    network_log: &'a NetworkHistoryLog,
    user_log: Option<MappedRwLockReadGuard<'a, ConcurrentLog<LogEntryId>>>,
    current_index: usize,
}

impl<'a> Iterator for UserHistoryLogIterator<'a> {
    type Item = &'a HistoryLogEntry;

    fn next(&mut self) -> Option<&'a HistoryLogEntry> {
        // Loop to ensure we can skip over entry IDs that are missing from the log.
        // Returning `None` in that case would signal the end of the iteration; we
        // only want to do that if the id iterator is exhausted.
        loop {
            self.current_index += 1;
            let next_id = self.user_log.as_ref()?.get(self.current_index)?;
            let entry = self.network_log.entries.get(*next_id);
            if entry.is_some() {
                break entry;
            }
        }
    }
}

pub struct ReverseUserHistoryLogIterator<'a> {
    network_log: &'a NetworkHistoryLog,
    user_log: Option<MappedRwLockReadGuard<'a, ConcurrentLog<LogEntryId>>>,
    current_index: usize,
}

impl<'a> Iterator for ReverseUserHistoryLogIterator<'a> {
    type Item = &'a HistoryLogEntry;

    fn next(&mut self) -> Option<&'a HistoryLogEntry> {
        // Loop to ensure we can skip over entry IDs that are missing from the log.
        // Returning `None` in that case would signal the end of the iteration; we
        // only want to do that if the id iterator is exhausted.
        loop {
            if self.current_index == 0 {
                break None;
            }
            self.current_index -= 1;
            let next_id = self.user_log.as_ref()?.get(self.current_index)?;
            let entry = self.network_log.entries.get(*next_id);
            if entry.is_some() {
                break entry;
            }
        }
    }
}

impl NetworkHistoryLog {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            entries: ConcurrentLog::new(),
            user_logs: RwLock::new(HashMap::new()),
        }
    }

    pub fn entries_for_user(&self, user: UserId) -> UserHistoryLogIterator<'_> {
        let user_log = RwLockReadGuard::try_map(self.user_logs.read(), |logs| logs.get(&user)).ok();

        UserHistoryLogIterator {
            current_index: user_log.as_ref().map(|l| l.start_index()).unwrap_or(0),
            user_log,
            network_log: self,
        }
    }

    pub fn entries_for_user_reverse(&self, user: UserId) -> ReverseUserHistoryLogIterator<'_> {
        let user_log = RwLockReadGuard::try_map(self.user_logs.read(), |logs| logs.get(&user)).ok();

        ReverseUserHistoryLogIterator {
            current_index: user_log.as_ref().map(|l| l.size()).unwrap_or(0),
            user_log,
            network_log: self,
        }
    }

    /// Add an entry to the history log, iff it is one of the event types that we store for
    /// history purposes.
    pub fn add(
        &self,
        details: NetworkStateChange,
        source_event: EventId,
        timestamp: i64,
    ) -> Option<LogEntryId> {
        use NetworkStateChange::*;
        match details {
            NewUser(_)
            | UserModeChange(_)
            | UserAwayChange(_)
            | NewUserConnection(_)
            | UserConnectionDisconnected(_)
            | NewServer(_)
            | ServerQuit(_)
            | NewAuditLogEntry(_)
            | UserLoginChange(_)
            | ServicesUpdate(_)
            | HistoryServerUpdate(_)
            | EventComplete(_) => None,

            UserNickChange(_)
            | UserQuit(_)
            | ChannelModeChange(_)
            | ChannelTopicChange(_)
            | ListModeAdded(_)
            | ListModeRemoved(_)
            | MembershipFlagChange(_)
            | ChannelJoin(_)
            | ChannelKick(_)
            | ChannelPart(_)
            | ChannelInvite(_)
            | ChannelRename(_)
            | NewMessage(_) => Some(self.entries.push_with_index(
                HistoryLogEntry {
                    id: 0,
                    source_event,
                    timestamp,
                    details,
                },
                |entry, index| entry.id = index,
            )),
        }
    }

    pub fn get(&self, entry_id: LogEntryId) -> Option<&HistoryLogEntry> {
        self.entries.get(entry_id)
    }

    pub fn add_entry_for_user(&self, user_id: UserId, entry_id: LogEntryId) {
        let user_logs = self.user_logs.upgradable_read();
        match user_logs.get(&user_id) {
            Some(log) => {
                log.push(entry_id);
            }
            None => {
                let mut user_logs_write = RwLockUpgradableReadGuard::upgrade(user_logs);
                let log = user_logs_write.entry(user_id).or_default();
                log.push(entry_id);
            }
        };
    }

    /// Remove entries older than the given timestamp
    ///
    /// Note that this expiry operation is not exact; some older entries may remain
    pub fn expire_entries(&mut self, older_than: i64) {
        self.entries.trim(|entry| entry.timestamp < older_than);

        let new_first_index = self.entries.start_index();

        for user_log in self.user_logs.get_mut().values_mut() {
            user_log.trim(|id| id < &new_first_index);
        }
    }
}
