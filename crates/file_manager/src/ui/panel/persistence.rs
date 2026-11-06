//! The part of the panel state that survives a restart: the sidebar geometry
//! and the view preferences.
//!
//! Zed's workspace database only remembers *which* panel is active and whether
//! a dock is open (`workspace::persistence::model::DockData`), not the internal
//! state of a panel, so this is stored in the key-value store instead — the
//! same place the debugger panel and the agent panel keep theirs
//! (`crates/debugger_ui/src/persistence.rs`,
//! `crates/agent_ui/src/agent_panel.rs`).
//!
//! The state is global rather than per workspace: it describes how the user
//! likes the panel to look (sidebar width, sort order, view mode), not what a
//! particular project contains.
//!
//! Writes are debounced: dragging the sidebar splitter calls
//! [`FileManagerPanel::persist_state`] on every frame, and only the last value
//! of a burst should reach the database.

use std::time::Duration;

use db::kvp::KeyValueStore;
use gpui::{App, Context};
use serde::{Deserialize, Serialize};
use util::ResultExt;

use super::FileManagerPanel;
use super::layout::{MAX_SIDEBAR_SPLIT, MAX_SIDEBAR_WIDTH, MIN_SIDEBAR_SPLIT, MIN_SIDEBAR_WIDTH};
use crate::model::entries::SortMode;
use crate::ui::ox_theme::metrics;
use crate::ui::views::ViewMode;

/// Key of the panel state in the key-value store.
const STATE_KEY: &str = "file_manager_panel_state";

/// How long the panel waits after the last change before writing the state.
const WRITE_DELAY: Duration = Duration::from_millis(250);

/// The panel state written between sessions.
///
/// Every field is optional on read (`#[serde(default)]`), so a state saved by an
/// older build keeps working, and a partially hand-edited entry does not
/// discard the rest.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct PersistedState {
    /// Whether the sidebar (places + folder tree) is shown.
    pub(crate) sidebar_visible: bool,
    /// Width of the sidebar in pixels.
    pub(crate) sidebar_width: f32,
    /// Share of the sidebar height given to the places list, between 0 and 1.
    pub(crate) sidebar_split: f32,
    /// Whether the details pane on the right is shown.
    pub(crate) details_pane_visible: bool,
    /// Whether dotfiles are listed.
    pub(crate) show_hidden: bool,
    /// How the entries are ordered.
    pub(crate) sort_mode: SortMode,
    /// The details view or the icon grid.
    pub(crate) view_mode: ViewMode,
    /// The index of the icon size in the grid view.
    pub(crate) icon_size: usize,
}

impl Default for PersistedState {
    fn default() -> Self {
        Self {
            sidebar_visible: true,
            sidebar_width: metrics::SIDEBAR_WIDTH,
            sidebar_split: 0.5,
            details_pane_visible: false,
            show_hidden: false,
            sort_mode: SortMode::default(),
            view_mode: ViewMode::default(),
            icon_size: metrics::DEFAULT_ICON_SIZE,
        }
    }
}

impl PersistedState {
    /// Reads the state of the previous session, or the defaults.
    pub(crate) fn load(cx: &App) -> Self {
        read_state(&KeyValueStore::global(cx), STATE_KEY)
    }

    /// Constrains the values to the ranges the panel itself enforces, so a
    /// stale or hand-edited entry cannot make the sidebar unusable (a width of
    /// `0`, a split outside `0…1`, an icon size beyond the last step).
    fn clamped(mut self) -> Self {
        if !self.sidebar_width.is_finite() {
            self.sidebar_width = metrics::SIDEBAR_WIDTH;
        }
        if !self.sidebar_split.is_finite() {
            self.sidebar_split = 0.5;
        }
        self.sidebar_width = self
            .sidebar_width
            .clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH);
        self.sidebar_split = self
            .sidebar_split
            .clamp(MIN_SIDEBAR_SPLIT, MAX_SIDEBAR_SPLIT);
        self.icon_size = self
            .icon_size
            .min(metrics::ICON_SIZES.len().saturating_sub(1));
        self
    }
}

/// Reads a persisted state. A missing key, a wrong database or an outdated
/// document all fall back to the defaults rather than failing the panel.
fn read_state(kvp: &KeyValueStore, key: &str) -> PersistedState {
    kvp.read_kvp(key)
        .log_err()
        .flatten()
        .and_then(|json| serde_json::from_str::<PersistedState>(&json).log_err())
        .unwrap_or_default()
        .clamped()
}

/// Writes a persisted state, logging (but not propagating) a database error.
async fn write_state(kvp: KeyValueStore, key: &str, state: PersistedState) {
    let Ok(json) = serde_json::to_string(&state) else {
        return;
    };
    kvp.write_kvp(key.to_string(), json).await.log_err();
}

impl FileManagerPanel {
    /// The state to write for the next session.
    pub(crate) fn persisted_state(&self) -> PersistedState {
        PersistedState {
            sidebar_visible: self.sidebar_visible,
            sidebar_width: self.sidebar_width,
            sidebar_split: self.sidebar_split,
            details_pane_visible: self.details_pane_visible,
            show_hidden: self.show_hidden,
            sort_mode: self.sort_mode,
            view_mode: self.view_mode,
            icon_size: self.icon_size,
        }
    }

    /// Schedules a write of the panel state.
    ///
    /// The write waits [`WRITE_DELAY`] and is cancelled by the next call, so a
    /// drag that changes the width every frame stores only its final value.
    /// The delay is short enough that a normal click-then-quit has landed by
    /// the time the process exits, and the state is a preference, not data, so
    /// the last few hundred milliseconds are not worth more machinery.
    pub(crate) fn persist_state(&mut self, cx: &mut Context<Self>) {
        let state = self.persisted_state();
        let kvp = KeyValueStore::global(cx);
        self._persist_task = Some(cx.spawn(async move |_, cx| {
            cx.background_executor().timer(WRITE_DELAY).await;
            write_state(kvp, STATE_KEY, state).await;
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_survives_a_json_round_trip() {
        let state = PersistedState {
            sidebar_visible: false,
            sidebar_width: 288.,
            sidebar_split: 0.3,
            details_pane_visible: true,
            show_hidden: true,
            sort_mode: SortMode::SizeDescending,
            view_mode: ViewMode::Grid,
            icon_size: 9,
        };

        let json = serde_json::to_string(&state).expect("state must serialize");
        let restored: PersistedState = serde_json::from_str(&json).expect("state must parse");

        assert_eq!(restored, state);
    }

    #[test]
    fn missing_fields_fall_back_to_the_defaults() {
        // A state written before the view preferences existed.
        let restored: PersistedState =
            serde_json::from_str(r#"{"sidebar_visible":false,"sidebar_width":300.0}"#)
                .expect("a partial state must still parse");
        let defaults = PersistedState::default();

        assert!(!restored.sidebar_visible);
        assert_eq!(restored.sidebar_width, 300.);
        assert_eq!(restored.sidebar_split, defaults.sidebar_split);
        assert_eq!(restored.sort_mode, defaults.sort_mode);
        assert_eq!(restored.view_mode, defaults.view_mode);
        assert_eq!(restored.icon_size, defaults.icon_size);
    }

    #[test]
    fn out_of_range_values_are_clamped() {
        // `read_state` clamps what it read, so a document that was edited by
        // hand (or written by a build with different limits) cannot break the
        // layout.
        let restored = serde_json::from_str::<PersistedState>(
            r#"{
                "sidebar_width": 5000.0,
                "sidebar_split": -2.0,
                "icon_size": 999
            }"#,
        )
        .expect("a state must parse")
        .clamped();

        assert_eq!(restored.sidebar_width, MAX_SIDEBAR_WIDTH);
        assert_eq!(restored.sidebar_split, MIN_SIDEBAR_SPLIT);
        assert_eq!(
            restored.icon_size,
            metrics::ICON_SIZES.len() - 1,
            "the icon size must stay a valid index"
        );

        let tiny = PersistedState {
            sidebar_width: 4.,
            ..PersistedState::default()
        }
        .clamped();
        assert_eq!(tiny.sidebar_width, MIN_SIDEBAR_WIDTH);
    }

    /// Regression guard: the persisted document must stay readable by a build
    /// that only knows the earlier fields, and vice versa.
    #[test]
    fn enum_names_are_stable() {
        let json = serde_json::to_string(&PersistedState {
            sort_mode: SortMode::ModifiedDescending,
            view_mode: ViewMode::Grid,
            ..PersistedState::default()
        })
        .expect("state must serialize");

        assert!(json.contains("\"modified_descending\""), "{json}");
        assert!(json.contains("\"grid\""), "{json}");
    }

    #[gpui::test]
    async fn state_round_trips_through_the_key_value_store(cx: &mut gpui::TestAppContext) {
        // A key of its own, so the test never disturbs the real panel state of
        // the machine running it.
        let key = format!("file_manager_panel_state_test_{}", std::process::id());
        let kvp = cx.update(|cx| KeyValueStore::global(cx));
        let state = PersistedState {
            sidebar_visible: false,
            sidebar_width: 321.,
            sidebar_split: 0.25,
            details_pane_visible: true,
            show_hidden: true,
            sort_mode: SortMode::ModifiedAscending,
            view_mode: ViewMode::Grid,
            icon_size: 2,
        };

        write_state(kvp.clone(), &key, state.clone()).await;
        assert_eq!(read_state(&kvp, &key), state.clamped());

        kvp.delete_kvp(key).await.expect("the test key must go");
    }
}
