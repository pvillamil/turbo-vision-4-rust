// (C) 2026 - Enzo Lombardi

//! Extra controls for the [turbo-vision](https://crates.io/crates/turbo-vision) TUI framework.
//!
//! Inspired by the classic third-party Turbo Vision add-on libraries
//! (TV Tool Box, tvDMX) and the modern tvision ecosystem, this crate adds
//! the controls the stock framework never shipped:
//!
//! - [`ComboBox`](combo_box::ComboBox) — input field with a drop-down list
//! - [`GridView`](grid::GridView) — multi-column data browser over a
//!   virtual [`RowProvider`](grid::RowProvider) (tvDMX-style)
//! - [`Gauge`](gauge::Gauge) — progress bar
//! - [`Slider`](slider::Slider) — horizontal value slider
//! - [`SpinControl`](spin::SpinControl) — numeric field with ▲/▼ steppers
//! - [`Notebook`](notebook::Notebook) — tabbed pages
//! - [`popup_menu`](popup_menu::popup_menu) — context menus at a point,
//!   plus check-mark menu item helpers
//! - [`VirtualListBox`](virtual_listbox::VirtualListBox) — list over a lazy
//!   [`ListProvider`](virtual_listbox::ListProvider), scales to millions of
//!   rows
//! - [`ScrollPane`](scroll_pane::ScrollPane) — scrolling interior for
//!   dialogs larger than the screen

pub mod combo_box;
pub mod gauge;
pub mod grid;
pub mod notebook;
pub mod popup_menu;
pub mod scroll_pane;
pub mod slider;
pub mod spin;
pub mod virtual_listbox;

pub use combo_box::ComboBox;
pub use gauge::Gauge;
pub use grid::{GridColumn, GridView, RowProvider, VecRowProvider};
pub use notebook::Notebook;
pub use popup_menu::{is_menu_item_checked, popup_menu, set_menu_item_checked};
pub use scroll_pane::ScrollPane;
pub use slider::Slider;
pub use spin::SpinControl;
pub use virtual_listbox::{ListProvider, VirtualListBox};
