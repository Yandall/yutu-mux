//! yutu-ui: Iced app shell. Full views land in Phase 3.
//! Kept dependency-free in Phase 0 so `cargo check` stays fast.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Route {
    #[default]
    Home,
    Search,
    Library,
    Settings,
}

#[derive(Debug, Default)]
pub struct AppState {
    pub route: Route,
    pub search_query: String,
}

impl AppState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn navigate(&mut self, route: Route) {
        self.route = route;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigate() {
        let mut s = AppState::new();
        s.navigate(Route::Search);
        assert_eq!(s.route, Route::Search);
    }
}
