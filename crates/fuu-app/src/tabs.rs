//! Browser-style tabs. Each tab is a *place*: a mailbox, what the reader shows, and
//! an optional draft that the right pane shows as the composer.
//!
//! Behaviour:
//! - Clicking a message changes the active tab's place; opening a message in a new
//!   tab (middle click) inserts it right after the active tab, unseen, in the
//!   background.
//! - New / reply / forward open a draft in a new tab; sending or discarding returns
//!   that tab to the place it was opened from.
//! - Closing the active tab activates the tab that takes its position (or the new
//!   last tab). Closing the only tab resets it to the home place.
//! - Closed tabs can be reopened at their original position.

/// Upper bound on open tabs.
pub const MAX_TABS: usize = 32;
/// Closed tabs remembered for reopening.
pub const MAX_CLOSED: usize = 16;

/// What the reader shows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Content {
    #[default]
    Nothing,
    /// A row in the current mailbox's message list.
    Message(usize),
    SecuritySample,
    CjkSample,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Draft {
    pub title: String,
    pub to: String,
    pub subject: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub mailbox: String,
    pub content: Content,
    pub draft: Option<Draft>,
    /// Opened in the background and not yet looked at.
    pub unseen: bool,
}

impl Place {
    pub fn home() -> Self {
        Self { mailbox: "受信トレイ".into(), content: Content::Nothing, draft: None, unseen: false }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TooManyTabs;

#[derive(Debug, Clone)]
pub struct Tabs {
    items: Vec<Place>,
    active: usize,
    closed: Vec<(usize, Place)>,
}

impl Default for Tabs {
    fn default() -> Self {
        Self { items: vec![Place::home()], active: 0, closed: Vec::new() }
    }
}

impl Tabs {
    pub fn items(&self) -> &[Place] {
        &self.items
    }

    pub fn active_index(&self) -> usize {
        self.active
    }

    pub fn active(&self) -> &Place {
        &self.items[self.active]
    }

    pub fn active_mut(&mut self) -> &mut Place {
        &mut self.items[self.active]
    }

    /// Insert a tab right after the active one.
    pub fn open(&mut self, mut place: Place, activate: bool) -> Result<usize, TooManyTabs> {
        if self.items.len() >= MAX_TABS {
            return Err(TooManyTabs);
        }
        let index = self.active + 1;
        place.unseen = !activate;
        self.items.insert(index, place);
        if activate {
            self.active = index;
        }
        Ok(index)
    }

    pub fn activate(&mut self, index: usize) {
        if index < self.items.len() {
            self.active = index;
            self.items[index].unseen = false;
        }
    }

    pub fn close(&mut self, index: usize) {
        if index >= self.items.len() {
            return;
        }
        if self.items.len() == 1 {
            let old = std::mem::replace(&mut self.items[0], Place::home());
            if old != Place::home() {
                self.remember(0, old);
            }
            self.active = 0;
            return;
        }
        let place = self.items.remove(index);
        self.remember(index, place);
        if index < self.active {
            self.active -= 1;
        } else if self.active >= self.items.len() {
            self.active = self.items.len() - 1;
        }
        self.items[self.active].unseen = false;
    }

    fn remember(&mut self, index: usize, place: Place) {
        self.closed.push((index, place));
        if self.closed.len() > MAX_CLOSED {
            self.closed.remove(0);
        }
    }

    /// Reopen the most recently closed tab at its original position.
    pub fn reopen(&mut self) -> Option<usize> {
        if self.items.len() >= MAX_TABS {
            return None;
        }
        let (index, mut place) = self.closed.pop()?;
        // A lone home tab is simply replaced.
        if self.items.len() == 1 && self.items[0] == Place::home() {
            place.unseen = false;
            self.items[0] = place;
            self.active = 0;
            return Some(0);
        }
        let index = index.min(self.items.len());
        place.unseen = false;
        self.items.insert(index, place);
        self.active = index;
        Some(index)
    }

    pub fn cycle(&mut self, delta: i32) {
        let len = self.items.len() as i32;
        let next = (self.active as i32 + delta).rem_euclid(len) as usize;
        self.activate(next);
    }

    /// Select by position; a negative index selects the last tab.
    pub fn select(&mut self, position: i32) {
        let index = if position < 0 { self.items.len() - 1 } else { position as usize };
        if index < self.items.len() {
            self.activate(index);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(i: usize) -> Place {
        Place { content: Content::Message(i), ..Place::home() }
    }

    #[test]
    fn starts_with_one_home_tab() {
        let t = Tabs::default();
        assert_eq!(t.items().len(), 1);
        assert_eq!(t.active(), &Place::home());
    }

    #[test]
    fn background_tabs_open_after_active_and_are_unseen() {
        let mut t = Tabs::default();
        t.open(message(1), false).unwrap();
        t.open(message(2), false).unwrap();
        assert_eq!(t.active_index(), 0);
        // Each background tab goes right after the active one.
        assert_eq!(t.items()[1].content, Content::Message(2));
        assert!(t.items()[1].unseen);
        t.activate(1);
        assert!(!t.active().unseen);
    }

    #[test]
    fn closing_active_tab_picks_the_one_taking_its_place() {
        let mut t = Tabs::default();
        t.open(message(1), true).unwrap();
        t.open(message(2), true).unwrap(); // [home, 1, 2], active 2
        t.activate(1);
        t.close(1); // [home, 2], the tab that took position 1 is active
        assert_eq!(t.active().content, Content::Message(2));
        t.close(1); // rightmost closed → new last tab
        assert_eq!(t.active_index(), 0);
    }

    #[test]
    fn closing_inactive_tab_keeps_active_tab() {
        let mut t = Tabs::default();
        t.open(message(1), true).unwrap();
        t.open(message(2), true).unwrap(); // active is index 2
        t.close(0);
        assert_eq!(t.active().content, Content::Message(2));
    }

    #[test]
    fn closing_last_tab_resets_to_home_and_reopen_restores() {
        let mut t = Tabs::default();
        t.active_mut().content = Content::Message(5);
        t.close(0);
        assert_eq!(t.items(), &[Place::home()]);
        assert_eq!(t.reopen(), Some(0));
        assert_eq!(t.active().content, Content::Message(5));
    }

    #[test]
    fn reopen_restores_original_position() {
        let mut t = Tabs::default();
        t.open(message(1), true).unwrap();
        t.open(message(2), true).unwrap(); // [home, 1, 2]
        t.close(1);
        assert_eq!(t.reopen(), Some(1));
        assert_eq!(t.items()[1].content, Content::Message(1));
        assert_eq!(t.active_index(), 1);
    }

    #[test]
    fn limits_are_enforced() {
        let mut t = Tabs::default();
        for i in 0..MAX_TABS - 1 {
            t.open(message(i), true).unwrap();
        }
        assert_eq!(t.open(message(99), true), Err(TooManyTabs));
        for _ in 0..MAX_CLOSED + 5 {
            t.close(t.active_index());
        }
        assert!(t.closed.len() <= MAX_CLOSED);
    }

    #[test]
    fn cycle_and_select_wrap() {
        let mut t = Tabs::default();
        t.open(message(1), true).unwrap();
        t.open(message(2), true).unwrap();
        t.cycle(1);
        assert_eq!(t.active_index(), 0);
        t.cycle(-1);
        assert_eq!(t.active_index(), 2);
        t.select(0);
        assert_eq!(t.active_index(), 0);
        t.select(-1);
        assert_eq!(t.active_index(), 2);
        t.select(7); // out of range: ignored
        assert_eq!(t.active_index(), 2);
    }
}
