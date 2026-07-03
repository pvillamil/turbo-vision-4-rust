# turbo-vision-extras

Extra controls for the [turbo-vision](https://crates.io/crates/turbo-vision)
TUI framework, inspired by the classic third-party Turbo Vision add-on
libraries (TV Tool Box, tvDMX) and the modern tvision ecosystem.

## Controls

| Control | Description |
|---|---|
| `ComboBox` | Input field with a drop-down list |
| `GridView` | Multi-column data browser over a lazy `RowProvider` (tvDMX style) |
| `Gauge` | Progress bar with optional percentage caption |
| `Slider` | Horizontal value slider (keyboard + mouse) |
| `SpinControl` | Numeric field with ▲/▼ steppers |
| `Notebook` | Tabbed pages (click tabs or Ctrl+PgUp/PgDn) |
| `popup_menu` | Context menu at a point, plus check-mark menu item helpers |
| `VirtualListBox` | List over a lazy `ListProvider` — scales to millions of rows |
| `ScrollPane` | Scrolling interior for dialogs larger than the screen |

## Example

```rust,no_run
use std::{cell::RefCell, rc::Rc};
use turbo_vision::core::geometry::Rect;
use turbo_vision::views::dialog::Dialog;
use turbo_vision_extras::{ComboBox, Gauge, SpinControl};

let mut dialog = Dialog::new(Rect::new(10, 4, 60, 18), "Settings");

let color = Rc::new(RefCell::new("Green".to_string()));
dialog.add(Box::new(ComboBox::new(
    Rect::new(2, 2, 24, 3),
    vec!["Red".into(), "Green".into(), "Blue".into()],
    color.clone(),
)));

let count = Rc::new(RefCell::new(4));
dialog.add(Box::new(SpinControl::new(Rect::new(2, 4, 12, 5), 1, 99, count.clone())));

let mut gauge = Gauge::new(Rect::new(2, 6, 46, 7), 100);
gauge.set_value(65);
dialog.add(Box::new(gauge));
```

## License

MIT, same as turbo-vision.

## Examples

```sh
cargo run -p turbo-vision-extras --example extras_controls  # ComboBox, SpinControl, Slider, Gauge
cargo run -p turbo-vision-extras --example extras_data      # GridView (100k rows), VirtualListBox (1M items)
cargo run -p turbo-vision-extras --example extras_notebook  # Notebook, ScrollPane, popup_menu (F9)
```
