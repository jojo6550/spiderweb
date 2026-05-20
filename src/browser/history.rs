//! Navigation history: back/forward stacks per tab.

/// Per-tab navigation history (back/forward stacks).
pub struct History {
    back: Vec<String>,
    forward: Vec<String>,
}

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
}

impl History {
    pub fn new() -> Self {
        Self { back: Vec::new(), forward: Vec::new() }
    }

    /// Push current URL onto back stack before navigating away.
    /// Clears the forward stack.
    pub fn push(&mut self, current_url: String) {
        self.forward.clear();
        self.back.push(current_url);
    }

    /// Go back: pops from back stack, pushes `current_url` onto forward.
    /// Returns the URL to navigate to, or `None` if at start.
    pub fn go_back(&mut self, current_url: &str) -> Option<String> {
        let prev = self.back.pop()?;
        self.forward.push(current_url.to_owned());
        Some(prev)
    }

    /// Go forward: pops from forward stack, pushes `current_url` onto back.
    /// Returns the URL to navigate to, or `None` if no forward history.
    pub fn go_forward(&mut self, current_url: &str) -> Option<String> {
        let next = self.forward.pop()?;
        self.back.push(current_url.to_owned());
        Some(next)
    }

    pub fn can_go_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn can_go_forward(&self) -> bool {
        !self.forward.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn back_forward_cycle() {
        let mut h = History::new();
        h.push("https://a.com".into());
        h.push("https://b.com".into());

        assert!(h.can_go_back());
        assert!(!h.can_go_forward());

        let prev = h.go_back("https://c.com");
        assert_eq!(prev.as_deref(), Some("https://b.com"));
        assert!(h.can_go_forward());

        let next = h.go_forward("https://b.com");
        assert_eq!(next.as_deref(), Some("https://c.com"));
    }

    #[test]
    fn new_navigate_clears_forward() {
        let mut h = History::new();
        h.push("https://a.com".into());
        h.go_back("https://b.com");
        assert!(h.can_go_forward());
        h.push("https://c.com".into()); // new navigation clears forward
        assert!(!h.can_go_forward());
    }

    #[test]
    fn empty_history_returns_none() {
        let mut h = History::new();
        assert!(h.go_back("https://x.com").is_none());
        assert!(h.go_forward("https://x.com").is_none());
    }
}
