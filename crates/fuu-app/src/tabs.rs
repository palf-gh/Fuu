//! Browser-style tabs for the right pane. A tab is something the right pane shows:
//! a message, a draft or the settings. The message list (mailbox) is independent of
//! tabs, and with nothing open there are no tabs at all.
//!
//! Behaviour:
//! - Opening a message from the list replaces the active tab when it is a viewer
//!   (message or sample); otherwise it opens a new tab after the active one.
//!   Middle click opens a message in an unseen background tab.
//! - New / reply / forward open a draft in a new tab; sending or discarding closes it.
//! - Settings is a single tab: opening it again activates the existing one.
//! - Closing the active tab activates the tab that takes its position (or the new
//!   last tab). Closing the last tab leaves no tabs.
//! - Closed tabs can be reopened at their original position.

/// Upper bound on open tabs.
pub const MAX_TABS: usize = 32;
/// Closed tabs remembered for reopening.
pub const MAX_CLOSED: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Draft {
    pub title: String,
    pub from: String,
    pub to: String,
    pub cc: String,
    pub bcc: String,
    pub subject: String,
    pub body: String,
    /// Whether the CC/BCC fields are expanded.
    pub show_cc_bcc: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TabContent {
    /// A row in the current mailbox's message list.
    Message(usize),
    SecuritySample,
    CjkSample,
    Draft(Draft),
    Settings,
}

impl TabContent {
    /// Viewer tabs are replaced when another message is opened from the list.
    pub fn is_viewer(&self) -> bool {
        matches!(self, Self::Message(_) | Self::SecuritySample | Self::CjkSample)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tab {
    pub content: TabContent,
    /// Opened in the background and not yet looked at.
    pub unseen: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TooManyTabs;

#[derive(Debug, Clone, Default)]
pub struct Tabs {
    items: Vec<Tab>,
    active: Option<usize>,
    closed: Vec<(usize, Tab)>,
}

impl Tabs {
    pub fn items(&self) -> &[Tab] {
        &self.items
    }

    pub fn active_index(&self) -> Option<usize> {
        self.active
    }

    pub fn active(&self) -> Option<&Tab> {
        self.active.map(|i| &self.items[i])
    }

    pub fn active_mut(&mut self) -> Option<&mut Tab> {
        self.active.map(|i| &mut self.items[i])
    }

    /// Insert a tab right after the active one (or at the end when none is active).
    pub fn open(&mut self, content: TabContent, activate: bool) -> Result<usize, TooManyTabs> {
        if self.items.len() >= MAX_TABS {
            return Err(TooManyTabs);
        }
        let index = self.active.map_or(self.items.len(), |a| a + 1);
        self.items.insert(index, Tab { content, unseen: !activate });
        // The new tab always lands after the active one, so the active index is stable.
        if activate {
            self.active = Some(index);
        }
        Ok(index)
    }

    /// Show a viewer in the active tab if it is a viewer, otherwise in a new tab.
    pub fn show(&mut self, content: TabContent) -> Result<usize, TooManyTabs> {
        match self.active {
            Some(a) if self.items[a].content.is_viewer() => {
                self.items[a] = Tab { content, unseen: false };
                Ok(a)
            }
            _ => self.open(content, true),
        }
    }

    /// Activate the settings tab, opening it if needed.
    pub fn show_settings(&mut self) -> Result<usize, TooManyTabs> {
        if let Some(i) = self.items.iter().position(|t| t.content == TabContent::Settings) {
            self.activate(i);
            return Ok(i);
        }
        self.open(TabContent::Settings, true)
    }

    pub fn activate(&mut self, index: usize) {
        if index < self.items.len() {
            self.active = Some(index);
            self.items[index].unseen = false;
        }
    }

    pub fn close(&mut self, index: usize) {
        if index >= self.items.len() {
            return;
        }
        let tab = self.items.remove(index);
        self.closed.push((index, tab));
        if self.closed.len() > MAX_CLOSED {
            self.closed.remove(0);
        }
        self.active = match self.active {
            _ if self.items.is_empty() => None,
            Some(a) if index < a => Some(a - 1),
            Some(a) => Some(a.min(self.items.len() - 1)),
            None => None,
        };
        if let Some(a) = self.active {
            self.items[a].unseen = false;
        }
    }

    /// Reopen the most recently closed tab at its original position.
    pub fn reopen(&mut self) -> Option<usize> {
        if self.items.len() >= MAX_TABS {
            return None;
        }
        let (index, mut tab) = self.closed.pop()?;
        let index = index.min(self.items.len());
        tab.unseen = false;
        self.items.insert(index, tab);
        self.active = Some(index);
        Some(index)
    }

    /// Move a tab to a new position, keeping the same tab active.
    pub fn move_tab(&mut self, from: usize, to: usize) {
        if from >= self.items.len() || to >= self.items.len() || from == to {
            return;
        }
        let tab = self.items.remove(from);
        self.items.insert(to, tab);
        self.active = self.active.map(|a| {
            if a == from {
                to
            } else if from < a && a <= to {
                a - 1
            } else if to <= a && a < from {
                a + 1
            } else {
                a
            }
        });
    }

    pub fn cycle(&mut self, delta: i32) {
        if self.items.is_empty() {
            return;
        }
        let len = self.items.len() as i32;
        let current = self.active.unwrap_or(0) as i32;
        self.activate((current + delta).rem_euclid(len) as usize);
    }

    /// Select by position; a negative index selects the last tab.
    pub fn select(&mut self, position: i32) {
        if self.items.is_empty() {
            return;
        }
        let index = if position < 0 { self.items.len() - 1 } else { position as usize };
        self.activate(index);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft() -> TabContent {
        TabContent::Draft(Draft::default())
    }

    #[test]
    fn starts_empty() {
        let t = Tabs::default();
        assert!(t.items().is_empty());
        assert_eq!(t.active(), None);
    }

    #[test]
    fn show_replaces_viewer_and_opens_after_drafts() {
        let mut t = Tabs::default();
        t.show(TabContent::Message(1)).unwrap();
        t.show(TabContent::Message(2)).unwrap();
        assert_eq!(t.items().len(), 1, "viewer replaced");
        assert_eq!(t.active().unwrap().content, TabContent::Message(2));
        t.open(draft(), true).unwrap();
        t.show(TabContent::Message(3)).unwrap();
        assert_eq!(t.items().len(), 3, "a draft is never replaced");
        assert_eq!(t.active_index(), Some(2));
    }

    #[test]
    fn background_tabs_are_unseen_and_keep_active() {
        let mut t = Tabs::default();
        t.show(TabContent::Message(1)).unwrap();
        t.open(TabContent::Message(2), false).unwrap();
        assert_eq!(t.active_index(), Some(0));
        assert!(t.items()[1].unseen);
        t.activate(1);
        assert!(!t.active().unwrap().unseen);
    }

    #[test]
    fn closing_picks_neighbour_and_last_close_leaves_nothing() {
        let mut t = Tabs::default();
        t.show(TabContent::Message(1)).unwrap();
        t.open(draft(), true).unwrap();
        t.open(TabContent::Settings, true).unwrap(); // [m1, draft, settings]
        t.activate(1);
        t.close(1);
        assert_eq!(t.active().unwrap().content, TabContent::Settings);
        t.close(1);
        assert_eq!(t.active_index(), Some(0));
        t.close(0);
        assert!(t.items().is_empty());
        assert_eq!(t.active(), None);
        assert_eq!(t.reopen(), Some(0));
        assert_eq!(t.active().unwrap().content, TabContent::Message(1));
    }

    #[test]
    fn settings_is_a_single_tab() {
        let mut t = Tabs::default();
        t.show_settings().unwrap();
        t.show(TabContent::Message(1)).unwrap();
        t.show_settings().unwrap();
        assert_eq!(t.items().iter().filter(|x| x.content == TabContent::Settings).count(), 1);
        assert_eq!(t.active().unwrap().content, TabContent::Settings);
    }

    #[test]
    fn move_tab_keeps_the_active_tab() {
        let mut t = Tabs::default();
        t.show(TabContent::Message(0)).unwrap();
        t.open(TabContent::Message(1), true).unwrap();
        t.open(TabContent::Message(2), true).unwrap(); // active 2
        t.move_tab(2, 0);
        assert_eq!(t.active_index(), Some(0));
        assert_eq!(t.active().unwrap().content, TabContent::Message(2));
        t.activate(2);
        t.move_tab(0, 2);
        assert_eq!(t.active().unwrap().content, TabContent::Message(1));
        t.move_tab(0, 9);
        assert_eq!(t.items().len(), 3);
    }

    #[test]
    fn limits_cycle_and_select() {
        let mut t = Tabs::default();
        t.cycle(1);
        t.select(0);
        for i in 0..MAX_TABS {
            t.open(TabContent::Message(i), true).unwrap();
        }
        assert_eq!(t.open(TabContent::Message(99), true), Err(TooManyTabs));
        t.select(0);
        t.cycle(-1);
        assert_eq!(t.active_index(), Some(MAX_TABS - 1));
        t.select(-1);
        assert_eq!(t.active_index(), Some(MAX_TABS - 1));
        for _ in 0..MAX_CLOSED + 5 {
            t.close(0);
        }
        assert!(t.closed.len() <= MAX_CLOSED);
    }
}
