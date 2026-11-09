use std::{
    collections::{HashMap, VecDeque},
    time::{Duration, Instant},
};

use protocol::{
    Command, ContextKind, OutputDevice, PlayContext, PlaybackStatus, PlayerState, QueueItem,
    RepeatMode, ReplayGain, ReplayGainMode, ReplayGainSettings, TrackRef,
};
use serde::{Deserialize, Serialize};

use crate::{EngineError, Playable, Resolver, audio::decoder};

/// "Previous" restarts the current track instead when past this point.
const RESTART_THRESHOLD: Duration = Duration::from_secs(3);
/// Entries remembered for "Previous".
const HISTORY: usize = 500;
/// Context entries published at once (see [`Player::context_window`]): small
/// contexts such as albums whole, big ones as a window that moves in steps.
const WINDOW: usize = 200;
const WINDOW_STEP: usize = 100;
const WINDOW_BEHIND: usize = 20;

/// Where the current entry came from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum Current {
    /// An index into the context.
    Context(usize),
    /// Taken off the queue (or out of history), so owned here.
    Item(Box<QueueItem>),
}

/// What plays after the current entry.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Step {
    /// The current entry again (repeat-one).
    Same,
    /// The front of the queue.
    Queue,
    Context(usize),
}

/// Where playback was, saved so a restarted engine comes back to it (see
/// [`Player::restore`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Playback {
    context: Vec<QueueItem>,
    context_meta: Option<(ContextKind, Option<String>)>,
    context_next: usize,
    queue: Vec<QueueItem>,
    current: Option<Current>,
    history: Vec<QueueItem>,
    position_ms: u64,
    #[serde(default)]
    unshuffled: Option<Vec<u64>>,
}

impl Playback {
    /// Moves the saved position to `position_ms` if `item` is still the
    /// current entry, so the position can be saved more often than the rest.
    pub fn seek(&mut self, item: u64, position_ms: u64) {
        let current = match &self.current {
            Some(Current::Context(i)) => self.context.get(*i),
            Some(Current::Item(item)) => Some(&**item),
            None => None,
        };
        if current.is_some_and(|current| current.id == item) {
            self.position_ms = position_ms;
        }
    }
}

/// Context, queue and transport state. Time is passed in and track resolution
/// goes through a [`Resolver`], so behaviour is deterministic under test.
///
/// The context is the list played from; the queue holds tracks added to play
/// before the context resumes, each removed once it starts.
#[derive(Debug)]
pub struct Player {
    context: Vec<QueueItem>,
    /// Kind and label, once something was played from a list.
    context_meta: Option<(ContextKind, Option<String>)>,
    /// Index of the context entry that plays once the queue is empty.
    context_next: usize,
    queue: Vec<QueueItem>,
    current: Option<Current>,
    /// What played before the current entry, oldest first.
    history: Vec<QueueItem>,
    /// While shuffle is on: the context's entry ids in the list's own order,
    /// to go back to. The context itself holds the shuffled play order.
    unshuffled: Option<Vec<u64>>,
    /// State of the random generator that shuffles.
    rng: u64,
    next_id: u64,
    status: PlaybackStatus,
    volume: f32,
    repeat: RepeatMode,
    /// Position when playback last started or was moved.
    base: Duration,
    /// Set while playing: the instant `base` was recorded.
    since: Option<Instant>,
    /// The current entry's id and what it resolved to, once resolved.
    resolved: Option<(u64, Playable)>,
    /// Counts seeks, so the audio output can tell when to follow one.
    seeks: u64,
    /// Decoder plugin names, in the order they are tried.
    decoders: Vec<String>,
    output: Option<String>,
    outputs: Vec<OutputDevice>,
    replay_gain: ReplayGainSettings,
}

impl Default for Player {
    fn default() -> Self {
        Self {
            context: Vec::new(),
            context_meta: None,
            context_next: 0,
            queue: Vec::new(),
            current: None,
            history: Vec::new(),
            unshuffled: None,
            // Any non-zero seed; the clock makes each run's order different.
            rng: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(1, |d| d.as_nanos() as u64 | 1),
            next_id: 1,
            status: PlaybackStatus::Stopped,
            volume: 1.0,
            repeat: RepeatMode::Off,
            base: Duration::ZERO,
            since: None,
            resolved: None,
            seeks: 0,
            decoders: decoder::all().iter().map(|p| p.name.to_string()).collect(),
            output: None,
            outputs: Vec::new(),
            replay_gain: ReplayGainSettings::default(),
        }
    }
}

impl Player {
    pub fn apply(&mut self, command: Command, now: Instant) -> Result<(), EngineError> {
        match command {
            Command::Play => self.play(now),
            Command::Pause => self.pause(now),
            Command::TogglePlayback => match self.status {
                PlaybackStatus::Playing => self.pause(now),
                _ => self.play(now),
            },
            Command::Stop => self.stop(),
            Command::Next => self.advance(now, false),
            Command::Previous => self.previous(now),
            Command::Seek { position_ms } => self.seek(Duration::from_millis(position_ms), now),
            Command::SetVolume { volume } => {
                if !(0.0..=1.0).contains(&volume) {
                    return Err(EngineError::InvalidVolume(volume));
                }
                self.volume = volume;
            }
            Command::SetRepeat { mode } => self.repeat = mode,
            Command::SetShuffle { on } => self.set_shuffle(on),
            Command::Enqueue { tracks } => {
                let items = self.entries(tracks);
                self.queue.extend(items);
            }
            Command::EnqueueNext { tracks } => {
                let items = self.entries(tracks);
                self.queue.splice(0..0, items);
            }
            Command::SetContext {
                tracks,
                start,
                kind,
                label,
            } => {
                if let Some(start) = start
                    && start as usize >= tracks.len()
                {
                    return Err(EngineError::IndexOutOfRange(start));
                }
                if let Some(old) = self.current.take() {
                    self.remember(old);
                }
                self.context = self.entries(tracks);
                self.context_meta = Some((kind, label));
                self.context_next = 0;
                self.queue.clear();
                let mut start = start;
                if self.unshuffled.is_some() {
                    self.unshuffled = Some(self.context.iter().map(|i| i.id).collect());
                    // The chosen entry plays first, then the rest in a random order.
                    if let Some(s) = start.replace(0) {
                        let first = self.context.remove(s as usize);
                        self.context.insert(0, first);
                        self.shuffle_from(1);
                    } else {
                        start = None;
                        self.shuffle_from(0);
                    }
                }
                match start {
                    Some(start) => {
                        self.go(Step::Context(start as usize), now);
                        self.play(now);
                    }
                    None => self.stop(),
                }
            }
            Command::UpdateContext { tracks } => self.update_context(tracks),
            Command::PlayItem { item_id } => {
                self.jump_to(item_id, now)?;
                self.play(now);
            }
            Command::RemoveItem { item_id } => self.remove(item_id, now)?,
            Command::MoveItem { item_id, to } => {
                let from = position(&self.queue, item_id)?;
                if to as usize >= self.queue.len() {
                    return Err(EngineError::IndexOutOfRange(to));
                }
                let item = self.queue.remove(from);
                self.queue.insert(to as usize, item);
            }
            Command::ClearQueue => self.queue.clear(),
            Command::SetDecoders { order } => {
                let known: Vec<_> = decoder::all().iter().map(|p| p.name).collect();
                for (i, name) in order.iter().enumerate() {
                    if !known.contains(&name.as_str()) || order[..i].contains(name) {
                        return Err(EngineError::BadDecoder(name.clone()));
                    }
                }
                self.decoders = order;
            }
            Command::SetOutput { device } => self.output = device,
            Command::ListOutputs => self.outputs = crate::audio::outputs(),
            Command::SetReplayGain { settings } => self.replay_gain = settings,
        }
        Ok(())
    }

    /// Advances past the end of a simulated (demo) track, going by its
    /// duration. Real tracks end when the audio output says so. Returns
    /// whether state changed.
    pub fn tick(&mut self, now: Instant) -> bool {
        if self.status != PlaybackStatus::Playing
            || matches!(self.playable(), Some(Playable::File(..)))
        {
            return false;
        }
        match self.current_duration() {
            Some(duration) if self.position(now) >= duration => {
                self.advance(now, true);
                true
            }
            _ => false,
        }
    }

    pub fn snapshot(&self, now: Instant, wall_ms: u64) -> PlayerState {
        PlayerState {
            status: self.status,
            volume: self.volume,
            repeat: self.repeat,
            shuffle: self.unshuffled.is_some(),
            position_ms: self.position(now).as_millis() as u64,
            sampled_at_ms: wall_ms,
            current: self.current_item().cloned(),
            context: self.context_meta.as_ref().map(|(kind, label)| PlayContext {
                kind: *kind,
                label: label.clone(),
                len: self.context.len() as u32,
                index: match self.current {
                    Some(Current::Context(i)) => Some(i as u32),
                    _ => None,
                },
                next: self.context_step().map(|i| i as u32),
            }),
            decoders: self.decoders.clone(),
            available_decoders: decoder::all().iter().map(|p| p.name.to_string()).collect(),
            output: self.output.clone(),
            outputs: self.outputs.clone(),
            replay_gain: self.replay_gain,
            gain_db: match self.playable() {
                Some(Playable::File(_, tags)) => self.gain_db(tags, false),
                _ => None,
            },
        }
    }

    /// Where playback is, to [`Self::restore`] after a restart.
    pub fn playback(&self, now: Instant) -> Playback {
        Playback {
            context: self.context.clone(),
            context_meta: self.context_meta.clone(),
            context_next: self.context_next,
            queue: self.queue.clone(),
            current: self.current.clone(),
            history: self.history.clone(),
            position_ms: self.position(now).as_millis() as u64,
            unshuffled: self.unshuffled.clone(),
        }
    }

    /// Goes back to `saved`, paused. Queued tracks that no longer resolve
    /// are dropped; a current one that doesn't is skipped once resolved.
    // shortcut: context entries aren't checked here (one lookup each, slow for big contexts); gone ones are marked and skipped when they come up.
    pub fn restore(&mut self, saved: Playback, resolver: &dyn Resolver) {
        let Playback {
            mut context,
            context_meta,
            context_next,
            mut queue,
            mut current,
            mut history,
            position_ms,
            unshuffled,
        } = saved;
        queue.retain(|item| resolver.resolve(&item.track).is_ok());
        if let Some(Current::Context(i)) = current
            && i >= context.len()
        {
            current = None;
        }
        let mut current_item = match &mut current {
            Some(Current::Item(item)) => Some(&mut **item),
            _ => None,
        };
        // Old errors may be fixed by now; entries are tried again when they come up.
        let entries = (current_item.iter_mut().map(|item| &mut **item))
            .chain(&mut context)
            .chain(&mut queue)
            .chain(&mut history);
        let mut last_id = 0;
        for item in entries {
            item.error = None;
            last_id = last_id.max(item.id);
        }
        self.next_id = last_id + 1;
        self.context_next = context_next.min(context.len());
        self.context = context;
        self.context_meta = context_meta;
        self.queue = queue;
        self.history = history;
        self.unshuffled = unshuffled;
        self.status = match current {
            Some(_) => PlaybackStatus::Paused,
            None => PlaybackStatus::Stopped,
        };
        self.current = current;
        self.base = Duration::from_millis(position_ms);
        self.since = None;
        self.resolved = None;
    }

    /// The queue: what plays before the context resumes.
    pub fn queue(&self) -> &[QueueItem] {
        &self.queue
    }

    /// The part of the context clients are sent, and its offset: the whole
    /// context up to [`WINDOW`] entries, else a window around the position
    /// that only moves every [`WINDOW_STEP`] entries, so a big context isn't
    /// resent on every track change.
    pub fn context_window(&self) -> (usize, &[QueueItem]) {
        let cursor = match self.current {
            Some(Current::Context(i)) => i,
            _ => self.context_next,
        };
        let offset = (cursor.saturating_sub(WINDOW_BEHIND) / WINDOW_STEP * WINDOW_STEP)
            .min(self.context.len().saturating_sub(WINDOW));
        let end = (offset + WINDOW).min(self.context.len());
        (offset, &self.context[offset..end])
    }

    pub fn status(&self) -> PlaybackStatus {
        self.status
    }

    pub fn volume(&self) -> f32 {
        self.volume
    }

    /// The enabled decoder plugins in order.
    pub fn decoders(&self) -> Vec<&'static decoder::Plugin> {
        let all = decoder::all();
        self.decoders
            .iter()
            .filter_map(|name| all.iter().find(|p| p.name == name).copied())
            .collect()
    }

    /// The chosen output device id, `None` for the system default.
    pub fn output(&self) -> Option<&str> {
        self.output.as_deref()
    }

    /// The ReplayGain adjustment in dB for a file with `tags` that is the
    /// current entry, or with `upcoming` the one after it.
    pub fn gain_db(&self, tags: &ReplayGain, upcoming: bool) -> Option<f32> {
        let from_context = if upcoming {
            matches!(self.step(true), Some(Step::Context(_) | Step::Same))
        } else {
            matches!(self.current, Some(Current::Context(_)))
        };
        let album = from_context && matches!(self.context_meta, Some((ContextKind::Album, _)));
        gain_db(&self.replay_gain, tags, album)
    }

    /// Whether ReplayGain is on, so untagged files are worth measuring.
    pub fn measures(&self) -> bool {
        self.replay_gain.mode != ReplayGainMode::Off
    }

    pub fn seeks(&self) -> u64 {
        self.seeks
    }

    pub fn current_item(&self) -> Option<&QueueItem> {
        match self.current.as_ref()? {
            Current::Context(i) => self.context.get(*i),
            Current::Item(item) => Some(&**item),
        }
    }

    /// The entry that plays when the current one ends naturally: the queue
    /// first, then the context, following repeat.
    pub fn upcoming(&self) -> Option<&QueueItem> {
        self.current.as_ref()?;
        match self.step(true)? {
            Step::Same => self.current_item(),
            Step::Queue => self.queue.first(),
            Step::Context(i) => self.context.get(i),
        }
    }

    /// Corrects the position to what the listener actually hears.
    pub fn sync_position(&mut self, position: Duration, now: Instant) {
        self.base = position;
        if self.since.is_some() {
            self.since = Some(now);
        }
    }

    /// The current track finished playing.
    pub fn track_ended(&mut self, now: Instant) {
        self.advance(now, true);
    }

    /// The audio output moved on to `item` by itself (gapless). Returns false
    /// if the entry has since left the queue and context.
    pub fn moved_to(&mut self, item: u64, playable: Playable, now: Instant) -> bool {
        if self.upcoming().is_some_and(|next| next.id == item) {
            self.advance(now, true);
        } else if self.jump_to(item, now).is_err() {
            return false;
        }
        self.resolved = Some((item, playable));
        true
    }

    /// `item` resolved but couldn't be played (unreadable, unsupported, no
    /// output device). It is marked and, if current, skipped.
    pub fn fail(&mut self, item: u64, reason: String, now: Instant) {
        let Some(entry) = self.entries_mut().find(|i| i.id == item) else {
            return;
        };
        entry.error = Some(reason);
        if self.current_item().map(|i| i.id) != Some(item) {
            return;
        }
        if self.step(false).is_none() || self.entries_mut().all(|i| i.error.is_some()) {
            self.stop();
        } else {
            self.advance(now, false);
        }
    }

    /// What the current entry resolved to, once [`Self::resolve_current`] has run.
    pub fn playable(&self) -> Option<&Playable> {
        let (id, playable) = self.resolved.as_ref()?;
        (self.current_item()?.id == *id).then_some(playable)
    }

    /// Resolves the current entry if it changed since the last call. Entries
    /// nothing can play get their reason recorded and are skipped; if none
    /// can play, playback stops. Returns whether state changed.
    pub fn resolve_current(&mut self, resolver: &dyn Resolver, now: Instant) -> bool {
        let mut changed = false;
        // Each entry is tried at most once per call, so entries nothing can
        // play can't loop forever under repeat.
        for _ in 0..=self.queue.len() + self.context.len() {
            let Some(item) = self.current_item() else {
                break;
            };
            let id = item.id;
            if self.resolved.as_ref().is_some_and(|(r, _)| *r == id) {
                return changed;
            }
            let track = item.track.clone();
            let mut result = resolver.resolve(&track);
            if self.measures()
                && matches!(&result, Ok(Playable::File(_, tags)) if tags.track_gain.is_none())
            {
                // Untagged and unmeasured: wait rather than change level mid-track.
                resolver.measure(std::slice::from_ref(&track), true);
                result = resolver.resolve(&track);
            }
            if let Some(Current::Context(i)) = self.current
                && matches!(self.context_meta, Some((ContextKind::Album, _)))
                && matches!(
                    self.replay_gain.mode,
                    ReplayGainMode::Album | ReplayGainMode::Auto
                )
            {
                // Album gain needs every track: measure the rest, in play order.
                let rest: Vec<_> = (self.context[i + 1..].iter())
                    .chain(&self.context[..i])
                    .map(|item| item.track.clone())
                    .collect();
                resolver.measure(&rest, false);
            }
            let Some(item) = self.current_item_mut() else {
                break;
            };
            match result {
                Ok(playable) => {
                    changed |= item.error.take().is_some();
                    self.resolved = Some((id, playable));
                    return changed;
                }
                Err(reason) => {
                    item.error = Some(reason);
                    changed = true;
                    if self.step(false).is_none() {
                        break;
                    }
                    self.advance(now, false);
                }
            }
        }
        if changed {
            // Ran out of entries to try.
            self.stop();
        }
        changed
    }

    pub fn position(&self, now: Instant) -> Duration {
        let position = match self.since {
            Some(since) => self.base + now.saturating_duration_since(since),
            None => self.base,
        };
        match self.current_duration() {
            Some(duration) => position.min(duration),
            None => position,
        }
    }

    fn current_duration(&self) -> Option<Duration> {
        let item = self.current_item()?;
        item.track.duration_ms.map(Duration::from_millis)
    }

    fn current_item_mut(&mut self) -> Option<&mut QueueItem> {
        match self.current.as_mut()? {
            Current::Context(i) => self.context.get_mut(*i),
            Current::Item(item) => Some(&mut **item),
        }
    }

    /// Every entry that can still play: the current one, the queue and the context.
    fn entries_mut(&mut self) -> impl Iterator<Item = &mut QueueItem> {
        let current = match &mut self.current {
            Some(Current::Item(item)) => Some(&mut **item),
            _ => None,
        };
        current
            .into_iter()
            .chain(&mut self.queue)
            .chain(&mut self.context)
    }

    /// Gives tracks entry ids.
    fn entries(&mut self, tracks: Vec<TrackRef>) -> Vec<QueueItem> {
        tracks.into_iter().map(|track| self.entry(track)).collect()
    }

    fn entry(&mut self, track: TrackRef) -> QueueItem {
        self.next_id += 1;
        QueueItem {
            id: self.next_id - 1,
            track,
            error: None,
        }
    }

    /// The context entry that plays once the queue is empty.
    fn context_step(&self) -> Option<usize> {
        if self.context_next < self.context.len() {
            Some(self.context_next)
        } else if self.repeat != RepeatMode::Off && !self.context.is_empty() {
            Some(0)
        } else {
            None
        }
    }

    /// What follows the current entry. `natural` is true when it ended on its
    /// own, the only case where repeat-one applies; repeat only applies to the
    /// context.
    fn step(&self, natural: bool) -> Option<Step> {
        if natural
            && self.repeat == RepeatMode::One
            && matches!(self.current, Some(Current::Context(_)))
        {
            Some(Step::Same)
        } else if !self.queue.is_empty() {
            Some(Step::Queue)
        } else {
            self.context_step().map(Step::Context)
        }
    }

    /// Moves to `step`, keeping the play/pause status. The entry left behind
    /// goes into history.
    fn go(&mut self, step: Step, now: Instant) {
        let next = match step {
            Step::Same => None,
            Step::Queue => Some(Current::Item(Box::new(self.queue.remove(0)))),
            Step::Context(i) => {
                self.context_next = i + 1;
                Some(Current::Context(i))
            }
        };
        if let Some(next) = next
            && let Some(old) = self.current.replace(next)
        {
            self.remember(old);
        }
        self.restart(now);
    }

    /// Shuffle on: what hasn't played yet takes a random order. Off: the
    /// list's order comes back around the current entry. Safe to repeat.
    // shortcut: repeat-all replays the same shuffled order; reshuffle on each wrap if wanted.
    fn set_shuffle(&mut self, on: bool) {
        match (on, self.unshuffled.take()) {
            (true, None) => {
                self.unshuffled = Some(self.context.iter().map(|i| i.id).collect());
                self.shuffle_from(self.context_next);
            }
            (false, Some(order)) => {
                let current = match self.current {
                    Some(Current::Context(i)) => Some(self.context[i].id),
                    _ => None,
                };
                let next = self.context.get(self.context_next).map(|i| i.id);
                let mut entries: HashMap<u64, QueueItem> =
                    self.context.drain(..).map(|i| (i.id, i)).collect();
                // Removed entries are missing from `entries` and drop out.
                self.context = order.iter().filter_map(|id| entries.remove(id)).collect();
                let at = |id| self.context.iter().position(|i| i.id == id);
                if let Some(i) = current.and_then(at) {
                    self.current = Some(Current::Context(i));
                    self.context_next = i + 1;
                } else {
                    self.context_next = next.and_then(at).unwrap_or(self.context.len());
                }
            }
            (_, kept) => self.unshuffled = kept,
        }
    }

    /// The list played from is now `tracks`: entries are matched by track
    /// (see [`Command::UpdateContext`]). Safe to repeat.
    fn update_context(&mut self, tracks: Vec<TrackRef>) {
        let mut old: Vec<Option<QueueItem>> = std::mem::take(&mut self.context)
            .into_iter()
            .map(Some)
            .collect();
        // The source's order: old entries by index, new tracks as new entries.
        let order: Vec<Result<usize, Box<QueueItem>>> = {
            // Each track's old entries, so a repeated track matches one each.
            let mut by_track: HashMap<(&str, &str), VecDeque<usize>> = HashMap::new();
            for (i, item) in old.iter().flatten().enumerate() {
                // All still there: `flatten` only unwraps.
                let key = (item.track.source.as_str(), item.track.id.as_str());
                by_track.entry(key).or_default().push_back(i);
            }
            let matched: Vec<_> = (tracks.iter())
                .map(|t| {
                    let key = (t.source.as_str(), t.id.as_str());
                    by_track.get_mut(&key).and_then(VecDeque::pop_front)
                })
                .collect();
            (tracks.into_iter().zip(matched))
                .map(|(track, i)| i.ok_or_else(|| Box::new(self.entry(track))))
                .collect()
        };
        let mut kept = vec![false; old.len()];
        for &i in order.iter().flatten() {
            kept[i] = true;
        }
        if let Some(Current::Context(i)) = self.current
            && !kept[i]
        {
            // Gone from the source, but it plays on by itself.
            let item = old[i].take().expect("not taken yet");
            self.current = Some(Current::Item(Box::new(item)));
        }
        let current = match self.current {
            Some(Current::Context(i)) => old[i].as_ref().map(|item| item.id),
            _ => None,
        };
        let played = self.context_next.min(old.len());
        if self.unshuffled.is_none() {
            let mut at = vec![None; old.len()];
            for (n, o) in order.iter().enumerate() {
                if let Ok(i) = o {
                    at[*i] = Some(n);
                }
            }
            // On after the last played entry that is still there.
            self.context_next = (0..played).rev().find_map(|i| at[i]).map_or(0, |n| n + 1);
            let entries = order.into_iter().map(|o| match o {
                Ok(i) => old[i].take().expect("each old entry is taken once"),
                Err(item) => *item,
            });
            self.context = entries.collect();
        } else {
            let ids = order.iter().map(|o| match o {
                Ok(i) => old[*i].as_ref().expect("kept").id,
                Err(item) => item.id,
            });
            self.unshuffled = Some(ids.collect());
            let mut added: Vec<_> = order
                .into_iter()
                .filter_map(|o| o.err().map(|b| *b))
                .collect();
            shuffle(&mut self.rng, &mut added);
            let played = (0..played).filter(|&i| kept[i]).count();
            let mut rest = kept.iter().filter(|&&k| k).count() - played;
            let mut left = old
                .into_iter()
                .zip(kept)
                .filter_map(|(o, k)| o.filter(|_| k));
            let mut context: Vec<_> = left.by_ref().take(played).collect();
            // What hasn't played keeps its order, new tracks fall at random among it.
            let mut fresh = added.len();
            let mut added = added.into_iter();
            while rest + fresh > 0 {
                if fresh > 0 && random(&mut self.rng, rest + fresh) < fresh {
                    context.extend(added.next());
                    fresh -= 1;
                } else {
                    context.extend(left.next());
                    rest -= 1;
                }
            }
            self.context = context;
            self.context_next = played;
        }
        if let Some(id) = current {
            let i = position(&self.context, id).expect("the current entry was kept");
            self.current = Some(Current::Context(i));
            self.context_next = i + 1;
        }
    }

    /// Puts the context's entries from `from` on in a random order.
    fn shuffle_from(&mut self, from: usize) {
        shuffle(&mut self.rng, &mut self.context[from..]);
    }

    fn remember(&mut self, entry: Current) {
        let item = match entry {
            Current::Context(i) => self.context.get(i).cloned(),
            Current::Item(item) => Some(*item),
        };
        self.history.extend(item);
        if self.history.len() > HISTORY {
            self.history.remove(0);
        }
    }

    /// Moves to the next entry, or stops if there is none.
    fn advance(&mut self, now: Instant, natural: bool) {
        match self.step(natural) {
            Some(step) => self.go(step, now),
            None => self.stop(),
        }
    }

    /// Steps back through history. The entry left goes back where it came
    /// from (the front of the queue, or the context position), so Next
    /// returns to it.
    fn previous(&mut self, now: Instant) {
        if self.position(now) > RESTART_THRESHOLD || self.history.is_empty() {
            self.seek(Duration::ZERO, now);
            return;
        }
        let item = self.history.pop().expect("checked above");
        match self.current.take() {
            Some(Current::Context(i)) => self.context_next = i,
            Some(Current::Item(left)) => self.queue.insert(0, *left),
            None => {}
        }
        // Back into the context if it is still there, else on its own.
        self.current = Some(match position(&self.context, item.id) {
            Ok(i) => {
                self.context_next = i + 1;
                Current::Context(i)
            }
            Err(_) => Current::Item(Box::new(item)),
        });
        self.restart(now);
    }

    /// Makes entry `id` current: from the queue (the rest of the queue
    /// stays) or the context (which moves there).
    fn jump_to(&mut self, id: u64, now: Instant) -> Result<(), EngineError> {
        if self.current_item().is_some_and(|i| i.id == id) {
            self.restart(now);
        } else if let Ok(i) = position(&self.queue, id) {
            let item = self.queue.remove(i);
            self.queue.insert(0, item);
            self.go(Step::Queue, now);
        } else {
            let i = position(&self.context, id)?;
            self.go(Step::Context(i), now);
        }
        Ok(())
    }

    fn remove(&mut self, id: u64, now: Instant) -> Result<(), EngineError> {
        if let Ok(i) = position(&self.queue, id) {
            self.queue.remove(i);
            return Ok(());
        }
        let current = self.current_item().is_some_and(|i| i.id == id);
        if let Ok(i) = position(&self.context, id) {
            self.context.remove(i);
            if self.context_next > i {
                self.context_next -= 1;
            }
            if let Some(Current::Context(c)) = &mut self.current
                && *c > i
            {
                *c -= 1;
            }
        } else if !current {
            return Err(EngineError::UnknownItem(id));
        }
        if current {
            // Like Next, without remembering the removed entry.
            self.current = None;
            match self.step(false) {
                Some(step) => self.go(step, now),
                None => self.stop(),
            }
        }
        Ok(())
    }

    fn play(&mut self, now: Instant) {
        if self.current.is_none()
            && let Some(step) = self.step(false)
        {
            self.go(step, now);
        }
        if self.current.is_some() && self.status != PlaybackStatus::Playing {
            self.status = PlaybackStatus::Playing;
            self.since = Some(now);
        }
    }

    fn pause(&mut self, now: Instant) {
        if self.status == PlaybackStatus::Playing {
            self.base = self.position(now);
            self.since = None;
            self.status = PlaybackStatus::Paused;
        }
    }

    fn stop(&mut self) {
        self.status = PlaybackStatus::Stopped;
        self.base = Duration::ZERO;
        self.since = None;
    }

    fn seek(&mut self, position: Duration, now: Instant) {
        self.seeks += 1;
        self.base = position;
        if self.since.is_some() {
            self.since = Some(now);
        }
        self.base = self.position(now);
    }

    /// Back to the start of the current entry, keeping the play/pause status.
    fn restart(&mut self, now: Instant) {
        self.base = Duration::ZERO;
        if self.since.is_some() {
            self.since = Some(now);
        }
    }
}

/// A random index below `n` (xorshift64: plenty for picking an order).
fn random(rng: &mut u64, n: usize) -> usize {
    *rng ^= *rng << 13;
    *rng ^= *rng >> 7;
    *rng ^= *rng << 17;
    (*rng % n as u64) as usize
}

/// Puts `items` in a random order (Fisher–Yates).
fn shuffle<T>(rng: &mut u64, items: &mut [T]) {
    for i in (1..items.len()).rev() {
        items.swap(i, random(rng, i + 1));
    }
}

fn position(items: &[QueueItem], id: u64) -> Result<usize, EngineError> {
    items
        .iter()
        .position(|item| item.id == id)
        .ok_or(EngineError::UnknownItem(id))
}

/// The ReplayGain adjustment in dB for a file with `tags`, `None` when off.
/// `album` says whether it plays from an album context (for Auto).
fn gain_db(settings: &ReplayGainSettings, tags: &ReplayGain, album: bool) -> Option<f32> {
    let track = (tags.track_gain, tags.track_peak);
    let album_values = (tags.album_gain, tags.album_peak);
    // Each falls back to the other when its tags are missing.
    let (first, second) = match settings.mode {
        ReplayGainMode::Off => return None,
        ReplayGainMode::Track => (track, album_values),
        ReplayGainMode::Album => (album_values, track),
        ReplayGainMode::Auto if album => (album_values, track),
        ReplayGainMode::Auto => (track, album_values),
    };
    let (Some(gain), peak) = (if first.0.is_some() { first } else { second }) else {
        return Some(settings.untagged_db);
    };
    let gain = gain + settings.preamp_db;
    Some(
        match peak.filter(|&p| settings.prevent_clipping && p > 0.0) {
            Some(peak) => gain.min(-20.0 * peak.log10()),
            None => gain,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(name: &str, duration_ms: u64) -> TrackRef {
        TrackRef {
            source: "demo".into(),
            id: format!("/{name}.flac"),
            title: Some(name.into()),
            duration_ms: Some(duration_ms),
            ..Default::default()
        }
    }

    fn set_context(p: &mut Player, tracks: &[(&str, u64)], start: Option<u32>, now: Instant) {
        let tracks = tracks.iter().map(|(n, d)| track(n, *d)).collect();
        let command = Command::SetContext {
            tracks,
            start,
            kind: ContextKind::List,
            label: None,
        };
        p.apply(command, now).unwrap();
    }

    /// A stopped player with `tracks` as its context.
    fn player_with(tracks: &[(&str, u64)], t0: Instant) -> Player {
        let mut player = Player::default();
        set_context(&mut player, tracks, None, t0);
        player
    }

    /// A player playing the first of `names` (1 s each) as its context.
    fn playing(names: &[&str], t0: Instant) -> Player {
        let mut player = Player::default();
        let tracks: Vec<_> = names.iter().map(|n| (*n, 1000)).collect();
        set_context(&mut player, &tracks, Some(0), t0);
        player
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn name(item: Option<&QueueItem>) -> Option<&str> {
        item?.track.title.as_deref()
    }

    fn now_playing(p: &Player) -> Option<&str> {
        name(p.current_item())
    }

    fn queued(p: &Player) -> Vec<&str> {
        p.queue.iter().map(|i| name(Some(i)).unwrap()).collect()
    }

    fn id(p: &Player, title: &str) -> u64 {
        p.queue
            .iter()
            .chain(&p.context)
            .find(|i| name(Some(i)) == Some(title))
            .unwrap()
            .id
    }

    fn enqueue(p: &mut Player, names: &[&str], now: Instant) {
        let tracks = names.iter().map(|n| track(n, 1000)).collect();
        p.apply(Command::Enqueue { tracks }, now).unwrap();
    }

    fn enqueue_next(p: &mut Player, names: &[&str], now: Instant) {
        let tracks = names.iter().map(|n| track(n, 1000)).collect();
        p.apply(Command::EnqueueNext { tracks }, now).unwrap();
    }

    fn repeat(p: &mut Player, mode: RepeatMode) {
        p.apply(Command::SetRepeat { mode }, Instant::now())
            .unwrap();
    }

    /// Presses Next `n` times, collecting what plays (and what was upcoming
    /// before each press, which must agree).
    fn nexts(p: &mut Player, n: usize, now: Instant) -> Vec<String> {
        (0..n)
            .map(|_| {
                let upcoming = name(p.upcoming()).map(str::to_string);
                p.apply(Command::Next, now).unwrap();
                let current = now_playing(p).unwrap().to_string();
                assert_eq!(upcoming.as_deref(), Some(current.as_str()), "upcoming");
                current
            })
            .collect()
    }

    #[test]
    fn position_advances_only_while_playing() {
        let t0 = Instant::now();
        let mut p = player_with(&[("a", 10_000)], t0);
        p.apply(Command::Play, t0).unwrap();
        assert_eq!(now_playing(&p), Some("a"));
        assert_eq!(p.position(t0 + ms(1500)), ms(1500));

        p.apply(Command::Pause, t0 + ms(2000)).unwrap();
        assert_eq!(p.position(t0 + ms(9000)), ms(2000));

        p.apply(Command::Play, t0 + ms(9000)).unwrap();
        assert_eq!(p.position(t0 + ms(9500)), ms(2500));
    }

    #[test]
    fn seek_while_playing_and_paused() {
        let t0 = Instant::now();
        let mut p = player_with(&[("a", 10_000)], t0);
        p.apply(Command::Play, t0).unwrap();
        p.apply(Command::Seek { position_ms: 5000 }, t0 + ms(100))
            .unwrap();
        assert_eq!(p.position(t0 + ms(600)), ms(5500));

        p.apply(Command::Pause, t0 + ms(600)).unwrap();
        p.apply(
            Command::Seek {
                position_ms: 99_000,
            },
            t0 + ms(700),
        )
        .unwrap();
        assert_eq!(p.position(t0 + ms(800)), ms(10_000), "clamped to duration");
    }

    #[test]
    fn tracks_advance_at_end_and_stop_after_last() {
        let t0 = Instant::now();
        let mut p = player_with(&[("a", 1000), ("b", 1000)], t0);
        p.apply(Command::Play, t0).unwrap();

        assert!(p.tick(t0 + ms(1000)));
        assert_eq!(now_playing(&p), Some("b"));
        assert_eq!(p.position(t0 + ms(1200)), ms(200));

        assert!(p.tick(t0 + ms(2000)));
        assert_eq!(p.status, PlaybackStatus::Stopped);
        assert_eq!(now_playing(&p), Some("b"), "the last entry stays current");
        assert!(!p.tick(t0 + ms(5000)));
    }

    #[test]
    fn with_an_empty_queue_next_follows_the_context() {
        let t0 = Instant::now();
        let mut p = playing(&["a", "b", "c"], t0);
        assert!(p.queue.is_empty());
        assert_eq!(nexts(&mut p, 2, t0), ["b", "c"]);
        assert_eq!(p.upcoming(), None);
        p.apply(Command::Next, t0).unwrap();
        assert_eq!(p.status, PlaybackStatus::Stopped);
    }

    #[test]
    fn the_queue_plays_first_and_is_removed_once_played() {
        let t0 = Instant::now();
        let mut p = playing(&["a", "b", "c"], t0);
        enqueue(&mut p, &["x", "y"], t0);
        assert_eq!(nexts(&mut p, 1, t0), ["x"]);
        assert_eq!(queued(&p), ["y"], "x left the queue when it started");
        assert_eq!(nexts(&mut p, 3, t0), ["y", "b", "c"]);
        assert!(p.queue.is_empty());
    }

    #[test]
    fn enqueue_adds_to_the_end_and_enqueue_next_to_the_front() {
        let t0 = Instant::now();
        let mut p = playing(&["a", "b"], t0);
        enqueue(&mut p, &["x"], t0);
        enqueue(&mut p, &["y"], t0);
        enqueue_next(&mut p, &["n1", "n2"], t0);
        assert_eq!(queued(&p), ["n1", "n2", "x", "y"]);
        assert_eq!(name(p.upcoming()), Some("n1"));
        assert_eq!(now_playing(&p), Some("a"), "the current entry doesn't move");
    }

    #[test]
    fn enqueueing_with_nothing_playing_plays_the_queue() {
        let t0 = Instant::now();
        let mut p = Player::default();
        enqueue(&mut p, &["x", "y"], t0);
        p.apply(Command::Play, t0).unwrap();
        assert_eq!(now_playing(&p), Some("x"));
        assert_eq!(nexts(&mut p, 1, t0), ["y"]);
        assert_eq!(p.upcoming(), None);
    }

    #[test]
    fn the_queue_emptying_mid_context_resumes_the_context() {
        let t0 = Instant::now();
        let mut p = playing(&["a", "b", "c"], t0);
        enqueue(&mut p, &["x"], t0);
        p.tick(t0 + ms(1000));
        assert_eq!(now_playing(&p), Some("x"));
        assert!(p.queue.is_empty());
        assert_eq!(name(p.upcoming()), Some("b"));
        p.tick(t0 + ms(2000));
        assert_eq!(now_playing(&p), Some("b"));
        p.tick(t0 + ms(3000));
        assert_eq!(now_playing(&p), Some("c"));
    }

    #[test]
    fn previous_walks_back_through_history_including_the_queue() {
        let t0 = Instant::now();
        let mut p = playing(&["a", "b", "c"], t0);
        enqueue(&mut p, &["x"], t0);
        assert_eq!(nexts(&mut p, 2, t0), ["x", "b"]);

        p.apply(Command::Previous, t0).unwrap();
        assert_eq!(now_playing(&p), Some("x"));
        p.apply(Command::Previous, t0).unwrap();
        assert_eq!(now_playing(&p), Some("a"));
        assert_eq!(queued(&p), ["x"], "x is back in the queue");
        p.apply(Command::Previous, t0).unwrap();
        assert_eq!(now_playing(&p), Some("a"), "no history: restarts");

        assert_eq!(nexts(&mut p, 3, t0), ["x", "b", "c"], "the same way again");
    }

    #[test]
    fn previous_restarts_after_threshold() {
        let t0 = Instant::now();
        let mut p = player_with(&[("a", 60_000), ("b", 60_000)], t0);
        p.apply(Command::Play, t0).unwrap();
        p.apply(Command::Next, t0).unwrap();

        p.apply(Command::Previous, t0 + ms(5000)).unwrap();
        assert_eq!(now_playing(&p), Some("b"));
        assert_eq!(p.position(t0 + ms(5000)), Duration::ZERO);

        p.apply(Command::Previous, t0 + ms(6000)).unwrap();
        assert_eq!(now_playing(&p), Some("a"));
    }

    #[test]
    fn jumping_within_the_context_keeps_the_queue() {
        let t0 = Instant::now();
        let mut p = playing(&["a", "b", "c", "d", "e"], t0);
        enqueue(&mut p, &["x"], t0);
        let d = id(&p, "d");
        p.apply(Command::PlayItem { item_id: d }, t0).unwrap();
        assert_eq!(now_playing(&p), Some("d"));
        assert_eq!(queued(&p), ["x"]);
        assert_eq!(nexts(&mut p, 2, t0), ["x", "e"]);
        p.apply(Command::Previous, t0).unwrap();
        p.apply(Command::Previous, t0).unwrap();
        assert_eq!(now_playing(&p), Some("d"));
    }

    #[test]
    fn playing_a_queued_entry_keeps_the_rest_of_the_queue() {
        let t0 = Instant::now();
        let mut p = playing(&["a", "b"], t0);
        enqueue(&mut p, &["x", "y", "z"], t0);
        let y = id(&p, "y");
        p.apply(Command::PlayItem { item_id: y }, t0).unwrap();
        assert_eq!(now_playing(&p), Some("y"));
        assert_eq!(queued(&p), ["x", "z"]);
        assert_eq!(nexts(&mut p, 3, t0), ["x", "z", "b"]);
    }

    #[test]
    fn queue_entries_move_and_are_removed() {
        let t0 = Instant::now();
        let mut p = playing(&["a"], t0);
        enqueue(&mut p, &["x", "y", "z"], t0);
        let z = id(&p, "z");
        p.apply(Command::MoveItem { item_id: z, to: 0 }, t0)
            .unwrap();
        assert_eq!(queued(&p), ["z", "x", "y"]);
        p.apply(Command::MoveItem { item_id: z, to: 2 }, t0)
            .unwrap();
        assert_eq!(queued(&p), ["x", "y", "z"]);
        assert_eq!(
            p.apply(Command::MoveItem { item_id: z, to: 3 }, t0),
            Err(EngineError::IndexOutOfRange(3))
        );
        let a = id(&p, "a");
        assert_eq!(
            p.apply(Command::MoveItem { item_id: a, to: 0 }, t0),
            Err(EngineError::UnknownItem(a)),
            "only queue entries move"
        );

        let x = id(&p, "x");
        p.apply(Command::RemoveItem { item_id: x }, t0).unwrap();
        assert_eq!(queued(&p), ["y", "z"]);
        p.apply(Command::ClearQueue, t0).unwrap();
        assert!(p.queue.is_empty());
        assert_eq!(now_playing(&p), Some("a"), "clearing keeps the context");
        assert_eq!(p.context.len(), 1);
    }

    #[test]
    fn removing_context_entries_keeps_the_current_track() {
        let t0 = Instant::now();
        let mut p = playing(&["a", "b", "c", "d"], t0);
        p.apply(Command::Next, t0).unwrap();

        let a = id(&p, "a");
        p.apply(Command::RemoveItem { item_id: a }, t0).unwrap();
        assert_eq!(now_playing(&p), Some("b"));
        assert_eq!(name(p.upcoming()), Some("c"));

        let b = id(&p, "b");
        p.apply(Command::RemoveItem { item_id: b }, t0).unwrap();
        assert_eq!(now_playing(&p), Some("c"), "removing the current moves on");
        assert_eq!(p.status, PlaybackStatus::Playing);

        let (c, d) = (id(&p, "c"), id(&p, "d"));
        p.apply(Command::RemoveItem { item_id: d }, t0).unwrap();
        p.apply(Command::RemoveItem { item_id: c }, t0).unwrap();
        assert_eq!(p.current_item(), None);
        assert_eq!(p.status, PlaybackStatus::Stopped);
        assert_eq!(
            p.apply(Command::RemoveItem { item_id: c }, t0),
            Err(EngineError::UnknownItem(c))
        );
    }

    #[test]
    fn repeat_one_replays_the_context_track_only() {
        let t0 = Instant::now();
        let mut p = playing(&["a", "b"], t0);
        repeat(&mut p, RepeatMode::One);
        assert_eq!(name(p.upcoming()), Some("a"));
        p.tick(t0 + ms(1000));
        assert_eq!(now_playing(&p), Some("a"), "replays on natural end");
        p.apply(Command::Next, t0 + ms(1100)).unwrap();
        assert_eq!(now_playing(&p), Some("b"), "but Next still skips");

        enqueue(&mut p, &["x"], t0);
        p.apply(Command::Next, t0 + ms(1100)).unwrap();
        assert_eq!(now_playing(&p), Some("x"));
        assert_eq!(name(p.upcoming()), Some("a"), "queued tracks play once");
        p.tick(t0 + ms(2100));
        assert_eq!(now_playing(&p), Some("a"));
    }

    #[test]
    fn repeat_all_wraps_the_context_after_the_queue() {
        let t0 = Instant::now();
        let mut p = playing(&["a", "b"], t0);
        repeat(&mut p, RepeatMode::All);
        enqueue(&mut p, &["x"], t0);
        assert_eq!(nexts(&mut p, 5, t0), ["x", "b", "a", "b", "a"]);
        assert_eq!(p.status, PlaybackStatus::Playing);
    }

    #[test]
    fn a_new_context_replaces_the_old_and_clears_the_queue() {
        let t0 = Instant::now();
        let mut p = playing(&["a", "b"], t0);
        enqueue(&mut p, &["x"], t0);
        set_context(
            &mut p,
            &[("p", 1000), ("q", 1000), ("r", 1000)],
            Some(1),
            t0,
        );
        assert!(p.queue.is_empty());
        assert_eq!(now_playing(&p), Some("q"));
        assert_eq!(p.status, PlaybackStatus::Playing);
        assert_eq!(nexts(&mut p, 1, t0), ["r"]);

        p.apply(Command::Previous, t0).unwrap();
        p.apply(Command::Previous, t0).unwrap();
        assert_eq!(now_playing(&p), Some("a"), "history outlives the context");
        assert_eq!(nexts(&mut p, 1, t0), ["q"]);

        let tracks = vec![track("s", 1000)];
        let command = |start| Command::SetContext {
            tracks: tracks.clone(),
            start: Some(start),
            kind: ContextKind::Album,
            label: None,
        };
        assert_eq!(
            p.apply(command(1), t0),
            Err(EngineError::IndexOutOfRange(1))
        );
        assert_eq!(
            p.context.len(),
            3,
            "rejected command leaves the context alone"
        );
    }

    #[test]
    fn the_snapshot_describes_the_context() {
        let t0 = Instant::now();
        let mut p = Player::default();
        assert_eq!(p.snapshot(t0, 0).context, None);
        let command = Command::SetContext {
            tracks: vec![track("a", 1000), track("b", 1000)],
            start: Some(0),
            kind: ContextKind::Album,
            label: Some("Blue".into()),
        };
        p.apply(command, t0).unwrap();
        enqueue(&mut p, &["x"], t0);
        let context = PlayContext {
            kind: ContextKind::Album,
            label: Some("Blue".into()),
            len: 2,
            index: Some(0),
            next: Some(1),
        };
        assert_eq!(p.snapshot(t0, 0).context.as_ref(), Some(&context));

        p.apply(Command::Next, t0).unwrap();
        let state = p.snapshot(t0, 0);
        assert_eq!(name(state.current.as_ref()), Some("x"));
        assert_eq!(state.context.unwrap().index, None, "x isn't in the context");
    }

    fn shuffle(p: &mut Player, on: bool) {
        p.apply(Command::SetShuffle { on }, Instant::now()).unwrap();
    }

    /// Twenty one-second tracks named 0 to 19.
    fn twenty() -> Vec<String> {
        (0..20).map(|i| i.to_string()).collect()
    }

    fn sorted(mut names: Vec<String>) -> Vec<String> {
        names.sort_by_key(|n| n.parse::<u32>().unwrap());
        names
    }

    #[test]
    fn shuffle_keeps_the_current_track_and_shuffles_the_rest() {
        let t0 = Instant::now();
        let names = twenty();
        let mut p = playing(&names.iter().map(String::as_str).collect::<Vec<_>>(), t0);
        p.apply(Command::Next, t0).unwrap();
        shuffle(&mut p, true);
        assert!(p.snapshot(t0, 0).shuffle);
        assert_eq!(now_playing(&p), Some("1"));
        let rest = nexts(&mut p, 18, t0);
        assert_eq!(sorted(rest.clone()), names[2..], "each of the rest once");
        assert_ne!(rest, names[2..], "in another order");
        p.apply(Command::Next, t0).unwrap();
        assert_eq!(p.status, PlaybackStatus::Stopped, "then the context ends");
        let history: Vec<_> = p.history.iter().map(|i| name(Some(i)).unwrap()).collect();
        assert_eq!(history[..2], ["0", "1"], "what played stays played");
    }

    #[test]
    fn a_context_played_shuffled_starts_with_the_chosen_track() {
        let t0 = Instant::now();
        let names = twenty();
        let tracks: Vec<_> = names.iter().map(|n| (n.as_str(), 1000)).collect();
        let mut p = Player::default();
        shuffle(&mut p, true);
        set_context(&mut p, &tracks, Some(7), t0);
        assert_eq!(now_playing(&p), Some("7"));
        let rest = nexts(&mut p, 19, t0);
        let mut others = names.clone();
        others.remove(7);
        assert_eq!(sorted(rest.clone()), others);
        assert_ne!(rest, others);
    }

    #[test]
    fn shuffle_off_goes_back_to_list_order_from_the_current_track() {
        let t0 = Instant::now();
        let names = twenty();
        let tracks: Vec<_> = names.iter().map(|n| (n.as_str(), 1000)).collect();
        let mut p = Player::default();
        shuffle(&mut p, true);
        set_context(&mut p, &tracks, Some(0), t0);
        let order: Vec<_> = p.context.iter().map(|i| name(Some(i)).unwrap()).collect();
        assert_ne!(order, names, "shuffled while on");
        p.apply(Command::Next, t0).unwrap();
        let current: u32 = now_playing(&p).unwrap().parse().unwrap();
        shuffle(&mut p, false);
        assert!(!p.snapshot(t0, 0).shuffle);
        assert_eq!(now_playing(&p), Some(current.to_string().as_str()));
        let expected: Vec<_> = (current + 1..20).map(|i| i.to_string()).collect();
        assert_eq!(nexts(&mut p, expected.len(), t0), expected);
        shuffle(&mut p, false);
        assert_eq!(p.context.len(), 20, "turning it off twice changes nothing");
    }

    #[test]
    fn shuffle_survives_a_restart_and_can_still_be_undone() {
        let t0 = Instant::now();
        let names = twenty();
        let mut p = playing(&names.iter().map(String::as_str).collect::<Vec<_>>(), t0);
        shuffle(&mut p, true);
        p.apply(
            Command::RemoveItem {
                item_id: id(&p, "5"),
            },
            t0,
        )
        .unwrap();
        let mut r = reload(&p, t0, &FakeResolver);
        assert!(r.snapshot(t0, 0).shuffle);
        shuffle(&mut r, false);
        let expected: Vec<_> = (1..20).filter(|&i| i != 5).map(|i| i.to_string()).collect();
        assert_eq!(
            nexts(&mut r, expected.len(), t0),
            expected,
            "list order, without the removed entry"
        );
    }

    fn update(p: &mut Player, names: &[&str], now: Instant) {
        let tracks = names.iter().map(|n| track(n, 1000)).collect();
        p.apply(Command::UpdateContext { tracks }, now).unwrap();
    }

    fn context_names(p: &Player) -> Vec<&str> {
        p.context.iter().map(|i| name(Some(i)).unwrap()).collect()
    }

    #[test]
    fn an_updated_context_follows_the_source_without_interrupting_the_current_track() {
        let t0 = Instant::now();
        let mut p = playing(&["a", "b", "c"], t0);
        p.apply(Command::Next, t0).unwrap();
        let (b, c) = (id(&p, "b"), id(&p, "c"));
        update(&mut p, &["x", "a", "b", "y", "c", "z"], t0 + ms(400));
        assert_eq!(p.current_item().unwrap().id, b, "same entry");
        assert_eq!(p.position(t0 + ms(500)), ms(500), "still playing on");
        assert_eq!(id(&p, "c"), c, "kept entries keep their ids");
        assert_eq!(
            nexts(&mut p, 3, t0 + ms(500)),
            ["y", "c", "z"],
            "new tracks after the current one play, in the source's order"
        );
        let history: Vec<_> = p.history.iter().map(|i| name(Some(i)).unwrap()).collect();
        assert_eq!(history, ["a", "b", "y", "c"], "history stays");
    }

    #[test]
    fn removed_tracks_drop_but_the_current_one_plays_on() {
        let t0 = Instant::now();
        let mut p = playing(&["a", "b", "c", "d"], t0);
        p.apply(Command::Next, t0).unwrap();
        let b = id(&p, "b");
        update(&mut p, &["a", "d"], t0);
        assert_eq!(p.current_item().unwrap().id, b, "b plays on");
        assert_eq!(p.status, PlaybackStatus::Playing);
        assert_eq!(context_names(&p), ["a", "d"]);
        assert_eq!(nexts(&mut p, 1, t0), ["d"], "c is gone");
    }

    #[test]
    fn updating_twice_with_the_same_tracks_changes_nothing() {
        let t0 = Instant::now();
        let names = twenty();
        let mut p = playing(&names.iter().map(String::as_str).collect::<Vec<_>>(), t0);
        shuffle(&mut p, true);
        let mut more = names.clone();
        more.push("new".into());
        let more: Vec<_> = more.iter().map(String::as_str).collect();
        update(&mut p, &more, t0);
        let once = p.playback(t0);
        update(&mut p, &more, t0);
        assert_eq!(p.playback(t0), once);
    }

    #[test]
    fn shuffled_new_tracks_go_among_the_unplayed_and_played_tracks_stay_played() {
        let t0 = Instant::now();
        let names = twenty();
        let mut p = playing(&names.iter().map(String::as_str).collect::<Vec<_>>(), t0);
        shuffle(&mut p, true);
        let played = [vec!["0".to_string()], nexts(&mut p, 4, t0)].concat();
        let current = now_playing(&p).unwrap().to_string();
        // A track not yet played goes, five come.
        let gone = names.iter().find(|n| !played.contains(n)).unwrap().clone();
        let added: Vec<String> = (20..25).map(|i| i.to_string()).collect();
        let source: Vec<&str> = (names.iter().filter(|n| **n != gone))
            .chain(&added)
            .map(String::as_str)
            .collect();
        let upcoming: Vec<String> = context_names(&p)[p.context_next..]
            .iter()
            .map(|n| n.to_string())
            .collect();
        update(&mut p, &source, t0);
        assert_eq!(now_playing(&p), Some(current.as_str()));
        let rest = nexts(&mut p, 24 - played.len(), t0);
        let expected: Vec<String> = (source.iter().map(|n| n.to_string()))
            .filter(|n| !played.contains(n))
            .collect();
        assert_eq!(sorted(rest.clone()), sorted(expected), "each unplayed once");
        assert_ne!(
            rest[rest.len() - 5..],
            added[..],
            "new tracks aren't appended"
        );
        let kept: Vec<_> = rest.iter().filter(|n| !added.contains(n)).collect();
        let before: Vec<_> = (upcoming.iter()).filter(|n| **n != gone).collect();
        assert_eq!(kept, before, "what was up next keeps its order");
        p.apply(Command::Next, t0).unwrap();
        assert_eq!(p.status, PlaybackStatus::Stopped, "nothing played twice");

        shuffle(&mut p, false);
        assert_eq!(context_names(&p), source, "the source's order comes back");
    }

    /// Saves `p` and restores it into a new player through JSON, as the server does.
    fn reload(p: &Player, now: Instant, resolver: &dyn Resolver) -> Player {
        let json = serde_json::to_string(&p.playback(now)).unwrap();
        let mut restored = Player::default();
        restored.restore(serde_json::from_str(&json).unwrap(), resolver);
        restored
    }

    #[test]
    fn a_restored_player_comes_back_paused_where_it_was() {
        let t0 = Instant::now();
        let mut p = playing(&["a", "b", "c"], t0);
        p.apply(Command::Next, t0).unwrap();
        enqueue(&mut p, &["x", "y"], t0);
        p.apply(Command::Seek { position_ms: 400 }, t0).unwrap();
        let before = p.snapshot(t0 + ms(100), 0);

        let t1 = t0 + ms(5000);
        let mut r = reload(&p, t0 + ms(100), &FakeResolver);
        assert_eq!(r.status(), PlaybackStatus::Paused);
        assert_eq!(now_playing(&r), Some("b"));
        assert_eq!(
            r.position(t1),
            ms(500),
            "exactly where it was, and not moving"
        );
        assert_eq!(queued(&r), ["x", "y"]);
        assert_eq!(r.snapshot(t1, 0).context, before.context);

        r.apply(Command::Previous, t1).unwrap();
        assert_eq!(now_playing(&r), Some("a"), "history survives");
        assert_eq!(r.snapshot(t1, 0).context.unwrap().index, Some(0));
        assert_eq!(nexts(&mut r, 4, t1), ["x", "y", "b", "c"]);
    }

    #[test]
    fn a_queued_current_entry_is_restored() {
        let t0 = Instant::now();
        let mut p = playing(&["a", "b"], t0);
        enqueue(&mut p, &["x"], t0);
        p.apply(Command::Next, t0).unwrap();
        let mut r = reload(&p, t0, &FakeResolver);
        assert_eq!(now_playing(&r), Some("x"));
        assert_eq!(r.snapshot(t0, 0).context.unwrap().index, None);
        enqueue(&mut r, &["new"], t0);
        assert_ne!(
            id(&r, "new"),
            r.current_item().unwrap().id,
            "ids stay unique"
        );
        assert_eq!(nexts(&mut r, 2, t0), ["new", "b"]);
    }

    #[test]
    fn restoring_drops_tracks_that_are_gone() {
        let t0 = Instant::now();
        let mut p = playing(&["missing-a", "b"], t0);
        enqueue(&mut p, &["x", "missing-y", "z"], t0);
        let mut r = reload(&p, t0, &FakeResolver);
        assert_eq!(queued(&r), ["x", "z"]);
        r.resolve_current(&FakeResolver, t0);
        assert_eq!(
            now_playing(&r),
            Some("x"),
            "the current one is gone: the next"
        );
        assert_eq!(r.status(), PlaybackStatus::Paused);
    }

    #[test]
    fn a_later_position_applies_only_to_the_same_entry() {
        let t0 = Instant::now();
        let mut p = Player::default();
        set_context(&mut p, &[("a", 60_000), ("b", 60_000)], Some(0), t0);
        let a = id(&p, "a");
        let mut saved = p.playback(t0);
        saved.seek(a + 1, 9_000);
        let mut r = Player::default();
        r.restore(saved.clone(), &FakeResolver);
        assert_eq!(r.position(t0), ms(0), "another entry's position is ignored");
        saved.seek(a, 7_000);
        r.restore(saved, &FakeResolver);
        assert_eq!(r.position(t0), ms(7_000));
    }

    #[test]
    fn nothing_playing_restores_stopped() {
        let r = reload(&Player::default(), Instant::now(), &FakeResolver);
        assert_eq!(r.status(), PlaybackStatus::Stopped);
        assert!(r.current_item().is_none());
    }

    #[test]
    fn big_contexts_are_published_as_a_window() {
        let t0 = Instant::now();
        let names: Vec<String> = (0..1000).map(|i| i.to_string()).collect();
        let tracks: Vec<_> = names.iter().map(|n| (n.as_str(), 1000)).collect();
        let mut p = player_with(&tracks[..150], t0);
        assert_eq!(p.context_window().1.len(), 150, "small: all of it");

        p = player_with(&tracks, t0);
        let window = |p: &Player| {
            let (offset, items) = p.context_window();
            (offset, items.len())
        };
        assert_eq!(window(&p), (0, 200));
        for (at, offset) in [(119, 0), (120, 100), (219, 100), (999, 800)] {
            let item_id = id(&p, &at.to_string());
            p.apply(Command::PlayItem { item_id }, t0).unwrap();
            assert_eq!(window(&p), (offset, 200), "at {at}");
        }
    }

    #[test]
    fn invalid_volume_is_rejected() {
        let mut p = Player::default();
        assert_eq!(
            p.apply(Command::SetVolume { volume: 1.5 }, Instant::now()),
            Err(EngineError::InvalidVolume(1.5))
        );
    }

    /// Plays anything except tracks whose id contains "missing".
    struct FakeResolver;
    impl Resolver for FakeResolver {
        fn resolve(&self, track: &TrackRef) -> Result<Playable, String> {
            if track.id.contains("missing") {
                Err("not on this device".into())
            } else {
                Ok(Playable::Simulated)
            }
        }
    }

    #[test]
    fn unplayable_entries_are_marked_and_skipped() {
        let t0 = Instant::now();
        let mut p = player_with(&[("a", 1000), ("missing", 1000), ("c", 1000)], t0);
        let missing = id(&p, "missing");
        p.apply(Command::PlayItem { item_id: missing }, t0).unwrap();
        assert!(p.resolve_current(&FakeResolver, t0));
        assert_eq!(
            now_playing(&p),
            Some("c"),
            "skips to the next playable entry"
        );
        assert_eq!(p.context[1].error.as_deref(), Some("not on this device"));
        assert_eq!(p.playable(), Some(&Playable::Simulated));
        assert!(
            !p.resolve_current(&FakeResolver, t0),
            "resolves once per entry"
        );
    }

    #[test]
    fn a_context_nothing_can_play_stops() {
        let t0 = Instant::now();
        let mut p = player_with(&[("missing-1", 1000), ("missing-2", 1000)], t0);
        repeat(&mut p, RepeatMode::All);
        p.apply(Command::Play, t0).unwrap();
        p.resolve_current(&FakeResolver, t0);
        assert!(p.context.iter().all(|i| i.error.is_some()));
        assert_eq!(p.playable(), None);
        assert_ne!(p.status, PlaybackStatus::Playing);
    }

    #[test]
    fn gapless_moves_follow_the_queue() {
        let t0 = Instant::now();
        let mut p = playing(&["a", "b"], t0);
        enqueue(&mut p, &["x"], t0);
        let x = p.upcoming().unwrap().id;
        assert!(p.moved_to(x, Playable::Simulated, t0));
        assert_eq!(now_playing(&p), Some("x"));
        assert!(p.queue.is_empty());
        assert_eq!(p.playable(), Some(&Playable::Simulated));
        assert!(!p.moved_to(9999, Playable::Simulated, t0), "gone");
    }

    #[test]
    fn decoder_order_accepts_known_plugins_once() {
        let t0 = Instant::now();
        let mut p = Player::default();
        let set = |p: &mut Player, names: &[&str]| {
            let order = names.iter().map(|n| n.to_string()).collect();
            p.apply(Command::SetDecoders { order }, t0)
        };
        set(&mut p, &["symphonia", "libopus"]).unwrap();
        assert_eq!(
            p.decoders().iter().map(|d| d.name).collect::<Vec<_>>(),
            ["symphonia", "libopus"]
        );
        assert!(set(&mut p, &["symphonia", "symphonia"]).is_err());
        assert!(set(&mut p, &["vlc"]).is_err());
    }

    const TAGS: ReplayGain = ReplayGain {
        track_gain: Some(-6.0),
        track_peak: Some(0.5),
        album_gain: Some(-4.0),
        album_peak: Some(0.9),
    };

    #[test]
    fn auto_gain_uses_album_gain_only_in_an_album_context() {
        let t0 = Instant::now();
        let mut p = playing(&["a", "b"], t0);
        assert_eq!(
            p.gain_db(&TAGS, false),
            Some(-6.0),
            "a list uses track gain"
        );
        let tracks = vec![track("a", 1000), track("b", 1000)];
        let album = Command::SetContext {
            tracks,
            start: Some(0),
            kind: ContextKind::Album,
            label: None,
        };
        p.apply(album, t0).unwrap();
        assert_eq!(p.gain_db(&TAGS, false), Some(-4.0));
        assert_eq!(p.gain_db(&TAGS, true), Some(-4.0), "b follows in the album");
        enqueue(&mut p, &["x"], t0);
        assert_eq!(
            p.gain_db(&TAGS, true),
            Some(-6.0),
            "x is queued, not the album"
        );
        let x = p.upcoming().unwrap().id;
        p.moved_to(x, Playable::Simulated, t0);
        assert_eq!(p.gain_db(&TAGS, false), Some(-6.0));
    }

    /// Untagged files that get [`TAGS`] once measured, recording requests.
    #[derive(Default)]
    struct Measuring(std::sync::Mutex<Vec<(Vec<String>, bool)>>);
    impl Resolver for Measuring {
        fn resolve(&self, track: &TrackRef) -> Result<Playable, String> {
            let measured = self
                .0
                .lock()
                .unwrap()
                .iter()
                .any(|(ids, wait)| *wait && ids[0] == track.id);
            let tags = if measured {
                TAGS
            } else {
                ReplayGain::default()
            };
            Ok(Playable::File(track.id.clone().into(), tags))
        }
        fn measure(&self, tracks: &[TrackRef], wait: bool) {
            let ids = tracks.iter().map(|t| t.id.clone()).collect();
            self.0.lock().unwrap().push((ids, wait));
        }
    }

    #[test]
    fn an_untagged_track_waits_for_its_measurement_and_an_album_measures_the_rest() {
        let t0 = Instant::now();
        let mut p = Player::default();
        let tracks = vec![track("a", 1000), track("b", 1000), track("c", 1000)];
        let album = Command::SetContext {
            tracks,
            start: Some(1),
            kind: ContextKind::Album,
            label: None,
        };
        p.apply(album, t0).unwrap();
        let resolver = Measuring::default();
        p.resolve_current(&resolver, t0);
        assert_eq!(p.playable(), Some(&Playable::File("/b.flac".into(), TAGS)));
        let ids = |v: &[&str]| v.iter().map(|s| format!("/{s}.flac")).collect::<Vec<_>>();
        assert_eq!(
            *resolver.0.lock().unwrap(),
            [(ids(&["b"]), true), (ids(&["c", "a"]), false)],
            "waits for b, then measures the rest of the album in play order"
        );

        let mut p = playing(&["a"], t0);
        p.apply(
            Command::SetReplayGain {
                settings: ReplayGainSettings {
                    mode: ReplayGainMode::Off,
                    ..Default::default()
                },
            },
            t0,
        )
        .unwrap();
        let resolver = Measuring::default();
        p.resolve_current(&resolver, t0);
        assert!(
            resolver.0.lock().unwrap().is_empty(),
            "nothing is measured with ReplayGain off"
        );
    }

    #[test]
    fn gain_modes_fall_back_and_cap_by_peak() {
        let mut settings = ReplayGainSettings {
            mode: ReplayGainMode::Track,
            preamp_db: 10.0,
            prevent_clipping: false,
            untagged_db: -3.0,
        };
        assert_eq!(gain_db(&settings, &TAGS, true), Some(4.0));
        settings.mode = ReplayGainMode::Album;
        assert_eq!(gain_db(&settings, &TAGS, false), Some(6.0));
        // A peak of 0.9 allows about +0.92 dB.
        settings.prevent_clipping = true;
        let capped = gain_db(&settings, &TAGS, false).unwrap();
        assert!((capped - 0.915).abs() < 0.01, "{capped}");
        let only_track = ReplayGain {
            album_gain: None,
            album_peak: None,
            ..TAGS
        };
        assert_eq!(gain_db(&settings, &only_track, true), Some(4.0));
        assert_eq!(gain_db(&settings, &ReplayGain::default(), true), Some(-3.0));
        settings.mode = ReplayGainMode::Off;
        assert_eq!(gain_db(&settings, &TAGS, true), None);
    }
}
