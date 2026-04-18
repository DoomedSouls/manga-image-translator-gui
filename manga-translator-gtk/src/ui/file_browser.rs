// manga-translator-gtk/src/ui/file_browser.rs
//
// File browser widget — grid and list views for browsing manga images.
//
// Features:
//   - Gtk.GridView with Gtk.NoSelection (custom selection via GestureClick)
//   - Gtk.ListView as alternative (list mode)
//   - Background thread thumbnail loading with disk cache
//   - Folder rows with checkboxes for multi-folder selection
//   - Search/filter support
//   - Natural sort, name sort, date sort
//   - Selection state: .accent-selected / .folder-checked CSS classes
//   - Bounce/pop animations on selection

use adw::prelude::*;

use gtk::gio;
use gtk::glib;
use gtk::glib::clone;
use gtk::subclass::prelude::*;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use crate::config::ConfigManager;

use crate::i18n;
use crate::ipc_bridge::IpcBridge;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Replay the selection bounce/pop animation on a widget.
///
/// Removes and re-adds the `just-selected` CSS class to restart the animation.
/// A GLib timeout removes the class again after the animation duration (300ms).
fn play_selection_animation(widget: &gtk::Widget) {
    widget.remove_css_class("just-selected");
    // Force GTK to process the removal before re-adding
    gtk::glib::idle_add_local_once(clone!(
        #[weak]
        widget,
        move || {
            widget.add_css_class("just-selected");
            gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(300), move || {
                widget.remove_css_class("just-selected");
            });
        }
    ));
}

/// Recursively scan a folder for supported manga image files.
fn scan_folder_images(folder: &Path) -> Vec<PathBuf> {
    let mut result = Vec::new();
    scan_recursive(folder, &mut result);
    result
}

fn scan_recursive(dir: &Path, result: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_recursive(&path, result);
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            let ext = ext.to_lowercase();
            if matches!(
                ext.as_str(),
                "png" | "jpg" | "jpeg" | "bmp" | "tiff" | "tif" | "webp" | "gif"
            ) {
                result.push(path);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Data model for grid/list items
// ---------------------------------------------------------------------------

/// Custom GObject for GridView/ListView items.
///
/// Each item is either a folder or an image file. Folders show a folder
/// icon and can be toggled via checkbox; images show a thumbnail.
#[derive(Debug, Clone, PartialEq)]
pub enum BrowserItemType {
    Folder {
        path: PathBuf,
        name: String,
        image_count: usize,
    },
    Image {
        path: PathBuf,
        name: String,
        size_bytes: u64,
        modified: Option<u64>,
    },
}

impl BrowserItemType {
    pub fn display_name(&self) -> &str {
        match self {
            BrowserItemType::Folder { name, .. } => name,
            BrowserItemType::Image { name, .. } => name,
        }
    }

    pub fn path(&self) -> &Path {
        match self {
            BrowserItemType::Folder { path, .. } => path,
            BrowserItemType::Image { path, .. } => path,
        }
    }

    pub fn is_folder(&self) -> bool {
        matches!(self, BrowserItemType::Folder { .. })
    }

    pub fn is_image(&self) -> bool {
        matches!(self, BrowserItemType::Image { .. })
    }
}

// ---------------------------------------------------------------------------
// Selection state
// ---------------------------------------------------------------------------

/// Tracks which files and folders are currently selected.
///
/// This mirrors the Python GUI's approach:
/// - `selected_files`: individual image selections
/// - `checked_folders`: folder checkbox state (folder → set of contained images)
/// - CSS classes `.accent-selected` and `.folder-checked` provide visual feedback
#[derive(Debug, Clone, Default)]
pub struct SelectionState {
    /// Individual selected image paths.
    pub selected_files: HashSet<PathBuf>,
    /// Checked folder paths → set of image paths within that folder.
    pub checked_folders: HashMap<PathBuf, HashSet<PathBuf>>,
}

impl SelectionState {
    pub fn is_file_selected(&self, path: &Path) -> bool {
        self.selected_files.contains(path)
    }

    pub fn is_folder_checked(&self, path: &Path) -> bool {
        self.checked_folders.contains_key(path)
    }

    pub fn toggle_file(&mut self, path: &PathBuf) -> bool {
        if self.selected_files.contains(path) {
            self.selected_files.remove(path);
            false
        } else {
            self.selected_files.insert(path.clone());
            true
        }
    }

    pub fn toggle_folder(&mut self, folder_path: &PathBuf, image_paths: Vec<PathBuf>) -> bool {
        if self.checked_folders.contains_key(folder_path) {
            self.checked_folders.remove(folder_path);
            false
        } else {
            let set: HashSet<PathBuf> = image_paths.into_iter().collect();
            self.checked_folders.insert(folder_path.clone(), set);
            true
        }
    }

    pub fn select_all(&mut self, all_paths: &[PathBuf]) {
        for p in all_paths {
            self.selected_files.insert(p.clone());
        }
    }

    pub fn deselect_all(&mut self) {
        self.selected_files.clear();
        self.checked_folders.clear();
    }

    /// Get all selected file paths (individual + from checked folders).
    /// Results are sorted with natural/alphanumeric order
    /// (e.g. 0001.png, 0002.png, 0003.png).
    pub fn all_selected_files(&self) -> Vec<PathBuf> {
        let mut result: Vec<PathBuf> = self.selected_files.iter().cloned().collect();
        for paths in self.checked_folders.values() {
            for p in paths {
                if !result.contains(p) {
                    result.push(p.clone());
                }
            }
        }
        result.sort_by(|a, b| {
            alphanumeric_sort::compare_str(&a.to_string_lossy(), &b.to_string_lossy())
        });
        result
    }

    pub fn selected_count(&self) -> usize {
        let individual = self.selected_files.len();
        let from_folders: usize = self.checked_folders.values().map(|s| s.len()).sum();
        // Avoid double-counting files that are in both sets
        individual + from_folders
    }
}

// ---------------------------------------------------------------------------
// Thumbnail cache
// ---------------------------------------------------------------------------

/// In-memory thumbnail cache (path → Texture).
///
/// Thumbnails are loaded asynchronously via the Python bridge
/// (PIL/Pillow for decoding) and cached here.
#[derive(Debug, Default)]
pub struct ThumbnailCache {
    textures: HashMap<PathBuf, gtk::gdk::Texture>,
    loading: HashSet<PathBuf>,
}

impl ThumbnailCache {
    pub fn get(&self, path: &Path) -> Option<gtk::gdk::Texture> {
        self.textures.get(path).cloned()
    }

    pub fn insert(&mut self, path: PathBuf, texture: gtk::gdk::Texture) {
        self.textures.insert(path, texture);
    }

    pub fn is_loading(&self, path: &Path) -> bool {
        self.loading.contains(path)
    }

    pub fn set_loading(&mut self, path: &PathBuf, loading: bool) {
        if loading {
            self.loading.insert(path.clone());
        } else {
            self.loading.remove(path);
        }
    }

    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.textures.clear();
        self.loading.clear();
    }

    /// Clear only the loading state, keeping cached textures.
    /// Called on directory change so cached thumbnails survive navigation.
    pub fn clear_loading(&mut self) {
        self.loading.clear();
    }
}

// ---------------------------------------------------------------------------
// Thumbnail disk cache
// ---------------------------------------------------------------------------

/// Get the thumbnail disk cache directory.
fn thumbnail_cache_dir() -> PathBuf {
    let dir = ConfigManager::cache_dir().join("thumbnails");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Compute a cache file path for a given source image.
/// Uses the source path + mtime as the cache key, so modified files
/// automatically get new thumbnails.
fn thumbnail_cache_path(source_path: &Path) -> Option<PathBuf> {
    let mtime = std::fs::metadata(source_path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());

    let mtime = mtime?;
    let mut hasher = DefaultHasher::new();
    source_path.to_string_lossy().hash(&mut hasher);
    mtime.hash(&mut hasher);
    let hash = format!("{:016x}", hasher.finish());

    Some(thumbnail_cache_dir().join(format!("{}.png", hash)))
}

/// Try to load a cached thumbnail from disk. Returns PNG bytes if found.
fn load_cached_thumbnail(source_path: &Path) -> Option<Vec<u8>> {
    let cache_path = thumbnail_cache_path(source_path)?;
    std::fs::read(&cache_path).ok().filter(|b| !b.is_empty())
}

/// Save thumbnail PNG bytes to the disk cache.
fn save_cached_thumbnail(source_path: &Path, png_bytes: &[u8]) {
    let Some(cache_path) = thumbnail_cache_path(source_path) else {
        return;
    };
    if let Some(parent) = cache_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&cache_path, png_bytes);
}

// ---------------------------------------------------------------------------
// Directory scan cache
// ---------------------------------------------------------------------------

/// Cached directory scan results for instant re-display.
///
/// Only populated when the scan was done without an active search filter,
/// so the cached items represent the full unfiltered directory contents.
struct DirCacheEntry {
    /// All items (folders + images) with final image counts from Phase 2.
    items: Vec<BrowserItemType>,
    /// All image file paths (for select all).
    all_image_paths: Vec<PathBuf>,
}

// ---------------------------------------------------------------------------
// FileBrowser widget
// ---------------------------------------------------------------------------

/// Internal state for the FileBrowser widget.
pub struct FileBrowserState {
    /// Current directory being browsed.
    current_directory: PathBuf,
    /// All items in the current view.
    items: Vec<BrowserItemType>,
    /// All image file paths (for select all).
    all_image_paths: Vec<PathBuf>,
    /// Current selection state.
    selection: SelectionState,
    /// Thumbnail cache.
    thumbnails: ThumbnailCache,
    /// Current view mode ("grid" or "list").
    view_mode: String,
    /// Current search filter text.
    search_text: String,
    /// Python bridge for thumbnail generation.
    bridge: Arc<IpcBridge>,
    /// Sort method index (0=Name, 1=Natural, 2=Date).
    sort_method: u32,
    /// Grid item widget map (for applying CSS classes to specific items).
    grid_item_widgets: HashMap<PathBuf, gtk::Widget>,
    /// List item widget map.
    list_item_widgets: HashMap<PathBuf, gtk::Widget>,
    /// Callback when selection changes.
    on_selection_changed: Option<Rc<dyn Fn(&SelectionState)>>,
    /// Callback when a folder is activated (double-clicked).
    on_folder_activated: Option<Rc<dyn Fn(&Path)>>,
    /// Callback when files/folders are dropped onto the browser.
    on_files_dropped: Option<Rc<dyn Fn(&Path)>>,
    /// Directory scan cache: path → (unfiltered items, all_image_paths).
    /// Populated after Phase 2 completes. Enables instant re-display when
    /// navigating back to a previously visited directory.
    dir_cache: HashMap<PathBuf, DirCacheEntry>,
    /// Whether the last navigation was forward (into a child directory).
    nav_forward: bool,
}

// The actual FileBrowser GObject
glib::wrapper! {
    pub struct FileBrowser(ObjectSubclass<FileBrowserPrivate>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

/// GObject subclass for FileBrowser.
pub struct FileBrowserPrivate {
    state: RefCell<FileBrowserState>,
    // Widget references
    container: RefCell<Option<gtk::Box>>,
    grid_view: RefCell<Option<gtk::GridView>>,
    list_view: RefCell<Option<gtk::ListView>>,
    grid_model: RefCell<Option<gio::ListStore>>,
    list_model: RefCell<Option<gio::ListStore>>,
    grid_selection: RefCell<Option<gtk::NoSelection>>,
    list_selection: RefCell<Option<gtk::NoSelection>>,
    stack: RefCell<Option<gtk::Stack>>,
    count_label: RefCell<Option<gtk::Label>>,
}

impl Default for FileBrowserPrivate {
    fn default() -> Self {
        Self {
            state: RefCell::new(FileBrowserState {
                current_directory: PathBuf::new(),
                items: Vec::new(),
                all_image_paths: Vec::new(),
                selection: SelectionState::default(),
                thumbnails: ThumbnailCache::default(),
                view_mode: "grid".to_string(),
                search_text: String::new(),
                bridge: Arc::new(IpcBridge::new()),
                sort_method: 1,
                grid_item_widgets: HashMap::new(),
                list_item_widgets: HashMap::new(),
                on_selection_changed: None,
                on_folder_activated: None,
                on_files_dropped: None,
                dir_cache: HashMap::new(),
                nav_forward: true,
            }),
            container: RefCell::new(None),
            grid_view: RefCell::new(None),
            list_view: RefCell::new(None),
            grid_model: RefCell::new(None),
            list_model: RefCell::new(None),
            grid_selection: RefCell::new(None),
            list_selection: RefCell::new(None),
            stack: RefCell::new(None),
            count_label: RefCell::new(None),
        }
    }
}

#[glib::object_subclass]
impl ObjectSubclass for FileBrowserPrivate {
    const NAME: &'static str = "MangaFileBrowser";
    type Type = FileBrowser;
    type ParentType = gtk::Widget;
}

impl ObjectImpl for FileBrowserPrivate {
    fn constructed(&self) {
        self.parent_constructed();
        let obj = self.obj();
        // FileBrowser subclasses gtk::Widget (a leaf), so we must provide
        // a layout manager so the internal vbox child gets size-allocated.
        obj.set_layout_manager(Some(gtk::BoxLayout::new(gtk::Orientation::Vertical)));
        self.build_ui(&obj);
    }

    fn dispose(&self) {
        // Release widget references
        if let Some(container) = self.container.borrow().as_ref() {
            container.unparent();
        }
    }
}

impl WidgetImpl for FileBrowserPrivate {}

impl FileBrowserPrivate {
    /// Build the file browser UI: a Gtk.Stack with grid and list views.
    fn build_ui(&self, obj: &FileBrowser) {
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);

        // Status/count label at the top
        let count_label = gtk::Label::new(Some(&i18n::t("Keine Dateien")));
        count_label.add_css_class("dim-label");
        count_label.add_css_class("caption");
        count_label.set_margin_start(8);
        count_label.set_margin_top(4);
        count_label.set_margin_bottom(4);
        count_label.set_xalign(0.0);
        i18n::register_label(&count_label, "Keine Dateien");
        *self.count_label.borrow_mut() = Some(count_label.clone());
        vbox.append(&count_label);

        // Stack to switch between grid and list views
        let stack = gtk::Stack::new();
        stack.set_vexpand(true);
        stack.set_hexpand(true);
        stack.set_transition_type(gtk::StackTransitionType::Crossfade);
        stack.set_transition_duration(200);

        // -- Grid View --
        let grid_model = gio::ListStore::new::<BrowserItem>();
        let grid_no_selection = gtk::NoSelection::new(Some(grid_model.clone()));

        let grid_factory = gtk::SignalListItemFactory::new();

        grid_factory.connect_setup(clone!(
            #[weak(rename_to = _this)]
            self,
            move |_factory, list_item| {
                let list_item = list_item.downcast_ref::<gtk::ListItem>().unwrap();
                let box_widget = gtk::Box::new(gtk::Orientation::Vertical, 4);
                box_widget.set_margin_start(4);
                box_widget.set_margin_end(4);
                box_widget.set_margin_top(4);
                box_widget.set_margin_bottom(4);
                box_widget.add_css_class("grid-item-box");
                box_widget.set_size_request(120, 140);

                // Thumbnail / icon — use gtk::Image so both icon names and
                // paintable textures work (gtk::Picture can't display themed icons)
                let image = gtk::Image::new();
                image.set_pixel_size(112);
                image.add_css_class("grid-item-picture");
                // Force fixed size so thumbnails don't cause variable cell sizes.
                // pixel_size only affects icons; paintables need explicit size_request.
                image.set_size_request(112, 112);
                image.set_halign(gtk::Align::Center);
                box_widget.append(&image);

                // Label (filename)
                let label = gtk::Label::new(None);
                label.set_max_width_chars(14);
                label.set_ellipsize(pango::EllipsizeMode::End);
                label.add_css_class("caption");
                box_widget.append(&label);

                list_item.set_child(Some(&box_widget));
            }
        ));

        grid_factory.connect_bind(clone!(
            #[weak(rename_to = this)]
            self,
            move |_factory, list_item| {
                let list_item = list_item.downcast_ref::<gtk::ListItem>().unwrap();
                let child = list_item.child().unwrap();
                let item = list_item.item().unwrap();
                let browser_item = item.downcast::<BrowserItem>().unwrap();
                let data = browser_item.data();

                let box_widget = child.downcast::<gtk::Box>().unwrap();

                // Get image and label from the box
                let image = box_widget
                    .first_child()
                    .unwrap()
                    .downcast::<gtk::Image>()
                    .unwrap();
                let label = box_widget
                    .last_child()
                    .unwrap()
                    .downcast::<gtk::Label>()
                    .unwrap();

                // Set label
                label.set_label(data.display_name());

                // Apply selection CSS
                let mut state = this.state.borrow_mut();
                let path = data.path().to_path_buf();

                if data.is_folder() {
                    if state.selection.is_folder_checked(data.path()) {
                        box_widget.add_css_class("folder-checked");
                    } else {
                        box_widget.remove_css_class("folder-checked");
                    }
                } else {
                    if state.selection.is_file_selected(data.path()) {
                        box_widget.add_css_class("accent-selected");
                    } else {
                        box_widget.remove_css_class("accent-selected");
                    }
                }

                // Track widget for CSS updates
                state
                    .grid_item_widgets
                    .insert(path.clone(), box_widget.clone().upcast());
                drop(state);

                // Set thumbnail or placeholder icon
                if data.is_folder() {
                    // Use a folder icon for folders
                    image.set_icon_name(Some("folder-symbolic"));
                    image.set_pixel_size(64);
                } else {
                    // Try cache first
                    let state = this.state.borrow();
                    if let Some(texture) = state.thumbnails.get(data.path()) {
                        image.set_paintable(Some(&texture));
                    } else {
                        // Placeholder — generic image icon
                        image.set_icon_name(Some("image-x-generic-symbolic"));
                        image.set_pixel_size(48);
                        box_widget.add_css_class("thumbnail-loading");

                        // Queue async thumbnail load
                        if !state.thumbnails.is_loading(data.path()) {
                            let path_for_thumb = data.path().to_path_buf();
                            drop(state);
                            this.load_thumbnail_async(&path_for_thumb);
                        }
                    }
                }
            }
        ));

        grid_factory.connect_unbind(clone!(
            #[weak(rename_to = this)]
            self,
            move |_factory, list_item| {
                let list_item = list_item.downcast_ref::<gtk::ListItem>().unwrap();
                let item = list_item.item().unwrap();
                let browser_item = item.downcast::<BrowserItem>().unwrap();
                let data = browser_item.data();
                let mut state = this.state.borrow_mut();
                state.grid_item_widgets.remove(data.path());
            }
        ));

        let grid_view = gtk::GridView::new(Some(grid_no_selection.clone()), Some(grid_factory));
        grid_view.set_min_columns(2);
        grid_view.set_max_columns(6);
        grid_view.set_single_click_activate(true);
        grid_view.add_css_class("file-grid-view");

        // GestureClick for right-click selection (folders and files)
        // Left single-click navigates into folders (via connect_activate below)
        let gesture = gtk::GestureClick::new();
        gesture.set_button(gtk::gdk::BUTTON_SECONDARY);
        gesture.connect_released(clone!(
            #[weak(rename_to = this)]
            self,
            move |gesture, n_press, x, y| {
                // Only handle single right-clicks
                if n_press != 1 {
                    return;
                }
                gesture.set_state(gtk::EventSequenceState::Claimed);

                let grid_view_opt = this.grid_view.borrow().clone();
                let Some(grid_view) = grid_view_opt else {
                    return;
                };
                let pos = grid_view.pick(x, y, gtk::PickFlags::DEFAULT);

                // Walk up the widget tree to find the grid-item-box
                let mut widget = pos;
                let mut found_box: Option<gtk::Box> = None;
                while let Some(w) = widget {
                    if w.has_css_class("grid-item-box") {
                        if let Ok(b) = w.clone().downcast::<gtk::Box>() {
                            found_box = Some(b);
                            break;
                        }
                    }
                    widget = w.parent();
                }
                let Some(box_widget) = found_box else { return };
                let box_as_widget: gtk::Widget = box_widget.clone().upcast();

                // Reverse-lookup: find the path for the clicked widget
                let found_path = {
                    let state = this.state.borrow();
                    state
                        .grid_item_widgets
                        .iter()
                        .find(|(_, w)| **w == box_as_widget)
                        .map(|(p, _)| p.clone())
                };
                let Some(path) = found_path else { return };

                if path.is_dir() {
                    // Toggle folder selection
                    let images = scan_folder_images(&path);
                    let mut state = this.state.borrow_mut();
                    let checked = state.selection.toggle_folder(&path, images);
                    drop(state);

                    let state = this.state.borrow();
                    if let Some(widget) = state.grid_item_widgets.get(&path) {
                        if checked {
                            widget.add_css_class("folder-checked");
                            play_selection_animation(widget);
                        } else {
                            widget.remove_css_class("folder-checked");
                        }
                    }
                    drop(state);
                    this.notify_selection_changed();
                } else {
                    // Toggle file selection
                    let mut state = this.state.borrow_mut();
                    let selected = state.selection.toggle_file(&path);
                    drop(state);

                    let state = this.state.borrow();
                    if let Some(widget) = state.grid_item_widgets.get(&path) {
                        if selected {
                            widget.add_css_class("accent-selected");
                            play_selection_animation(widget);
                        } else {
                            widget.remove_css_class("accent-selected");
                        }
                    }
                    drop(state);
                    this.notify_selection_changed();
                }
            }
        ));
        grid_view.add_controller(gesture.clone());

        // Activate on single left-click (navigate into folders)
        grid_view.connect_activate(clone!(
            #[weak(rename_to = this)]
            self,
            move |_grid_view, position| {
                let item_info: Option<(bool, PathBuf)> = {
                    let model_opt = this.grid_model.borrow();
                    if let Some(model) = model_opt.as_ref() {
                        model.item(position).map(|item| {
                            let browser_item = item.downcast::<BrowserItem>().unwrap();
                            let data = browser_item.data();
                            (data.is_folder(), data.path().to_path_buf())
                        })
                    } else {
                        None
                    }
                };

                let Some((is_folder, path)) = item_info else {
                    return;
                };

                if is_folder {
                    let cb = this.state.borrow().on_folder_activated.clone();
                    if let Some(cb) = cb {
                        cb(&path);
                    }
                }
            }
        ));

        let grid_scrolled = gtk::ScrolledWindow::new();
        grid_scrolled.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        grid_scrolled.set_child(Some(&grid_view));
        stack.add_named(&grid_scrolled, Some("grid"));

        // -- List View --
        let list_model = gio::ListStore::new::<BrowserItem>();
        let list_no_selection = gtk::NoSelection::new(Some(list_model.clone()));

        let list_factory = gtk::SignalListItemFactory::new();

        list_factory.connect_setup(move |_factory, list_item| {
            let list_item = list_item.downcast_ref::<gtk::ListItem>().unwrap();
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            row.set_margin_start(8);
            row.set_margin_end(8);
            row.set_margin_top(2);
            row.set_margin_bottom(2);
            row.add_css_class("file-row");

            // Icon
            let icon = gtk::Image::new();
            icon.set_pixel_size(32);
            icon.add_css_class("file-row-icon");
            row.append(&icon);

            // Text column (name + size)
            let text_box = gtk::Box::new(gtk::Orientation::Vertical, 2);
            text_box.set_hexpand(true);

            let name_label = gtk::Label::new(None);
            name_label.set_xalign(0.0);
            name_label.set_ellipsize(pango::EllipsizeMode::End);
            name_label.add_css_class("file-row-name");
            text_box.append(&name_label);

            let detail_label = gtk::Label::new(None);
            detail_label.set_xalign(0.0);
            detail_label.add_css_class("caption");
            detail_label.add_css_class("dim-label");
            text_box.append(&detail_label);

            row.append(&text_box);

            list_item.set_child(Some(&row));
        });

        list_factory.connect_bind(clone!(
            #[weak(rename_to = this)]
            self,
            move |_factory, list_item| {
                let list_item = list_item.downcast_ref::<gtk::ListItem>().unwrap();
                let child = list_item.child().unwrap();
                let item = list_item.item().unwrap();
                let browser_item = item.downcast::<BrowserItem>().unwrap();
                let data = browser_item.data();

                let row = child.downcast::<gtk::Box>().unwrap();
                let icon = row.first_child().unwrap().downcast::<gtk::Image>().unwrap();
                let text_box = icon.next_sibling().unwrap().downcast::<gtk::Box>().unwrap();
                let name_label = text_box
                    .first_child()
                    .unwrap()
                    .downcast::<gtk::Label>()
                    .unwrap();
                let detail_label = name_label
                    .next_sibling()
                    .unwrap()
                    .downcast::<gtk::Label>()
                    .unwrap();
                // Extract info before consuming data in match
                let path = data.path().to_path_buf();
                let is_folder = data.is_folder();

                // Name
                name_label.set_label(data.display_name());

                // Detail text
                match data {
                    BrowserItemType::Folder { image_count, .. } => {
                        icon.set_icon_name(Some("folder-symbolic"));
                        detail_label.set_label(&format!("{} {}", image_count, i18n::t("Bilder")));
                    }
                    BrowserItemType::Image {
                        size_bytes,
                        modified: _,
                        ..
                    } => {
                        icon.set_icon_name(Some("image-x-generic-symbolic"));
                        detail_label.set_label(&format_file_size(size_bytes));
                    }
                }

                // Selection state
                let mut state = this.state.borrow_mut();

                if is_folder {
                    let checked = state.selection.is_folder_checked(&path);
                    if checked {
                        row.add_css_class("folder-checked");
                    } else {
                        row.remove_css_class("folder-checked");
                    }
                } else {
                    if state.selection.is_file_selected(&path) {
                        row.add_css_class("accent-selected");
                    } else {
                        row.remove_css_class("accent-selected");
                    }
                }

                state
                    .list_item_widgets
                    .insert(path.clone(), row.clone().upcast());
                drop(state);

                // Set thumbnail or placeholder icon for images (like grid view)
                if !is_folder {
                    let state = this.state.borrow();
                    if let Some(texture) = state.thumbnails.get(&path) {
                        icon.set_paintable(Some(&texture));
                        icon.set_pixel_size(112);
                    } else {
                        // No cached texture — show shimmer while loading
                        row.add_css_class("thumbnail-loading");
                        if !state.thumbnails.is_loading(&path) {
                            drop(state);
                            this.load_thumbnail_async(&path);
                        }
                    }
                }

                // Staggered directional fade-in: set initial opacity, then animate in after delay
                let position = list_item.position();
                let delay = std::time::Duration::from_millis(20 * position.min(50) as u64);
                let row_clone = row.clone();
                let nav_forward = this.state.borrow().nav_forward;
                let direction_class = if nav_forward {
                    "row-appear-right"
                } else {
                    "row-appear-left"
                };
                // Remove any lingering animation classes from previous binds
                row_clone.remove_css_class("row-appear-right");
                row_clone.remove_css_class("row-appear-left");
                row.set_opacity(0.0);
                glib::timeout_add_local(delay, move || {
                    row_clone.set_opacity(1.0);
                    row_clone.add_css_class(direction_class);
                    glib::ControlFlow::Break
                });
            }
        ));

        list_factory.connect_unbind(clone!(
            #[weak(rename_to = this)]
            self,
            move |_factory, list_item| {
                let list_item = list_item.downcast_ref::<gtk::ListItem>().unwrap();
                let item = list_item.item().unwrap();
                let browser_item = item.downcast::<BrowserItem>().unwrap();
                let data = browser_item.data();
                let mut state = this.state.borrow_mut();
                state.list_item_widgets.remove(data.path());
            }
        ));

        let list_view = gtk::ListView::new(Some(list_no_selection.clone()), Some(list_factory));
        list_view.set_single_click_activate(true);

        // GestureClick for right-click selection in list view (matches grid view behavior)
        let list_gesture = gtk::GestureClick::new();
        list_gesture.set_button(gtk::gdk::BUTTON_SECONDARY);
        list_gesture.connect_released(clone!(
            #[weak(rename_to = this)]
            self,
            move |gesture, n_press, x, y| {
                // Only handle single right-clicks
                if n_press != 1 {
                    return;
                }
                gesture.set_state(gtk::EventSequenceState::Claimed);

                let list_view_opt = this.list_view.borrow().clone();
                let Some(list_view) = list_view_opt else {
                    return;
                };
                let pos = list_view.pick(x, y, gtk::PickFlags::DEFAULT);

                // Walk up the widget tree to find the file-row
                let mut widget = pos;
                let mut found_row: Option<gtk::Box> = None;
                while let Some(w) = widget {
                    if w.has_css_class("file-row") {
                        if let Ok(b) = w.clone().downcast::<gtk::Box>() {
                            found_row = Some(b);
                            break;
                        }
                    }
                    widget = w.parent();
                }
                let Some(row_widget) = found_row else { return };
                let row_as_widget: gtk::Widget = row_widget.clone().upcast();

                // Reverse-lookup: find the path for the clicked widget
                let found_path = {
                    let state = this.state.borrow();
                    state
                        .list_item_widgets
                        .iter()
                        .find(|(_, w)| **w == row_as_widget)
                        .map(|(p, _)| p.clone())
                };
                let Some(path) = found_path else { return };

                if path.is_dir() {
                    // Toggle folder selection
                    let images = scan_folder_images(&path);
                    let mut state = this.state.borrow_mut();
                    let checked = state.selection.toggle_folder(&path, images);
                    drop(state);

                    let state = this.state.borrow();
                    if let Some(widget) = state.list_item_widgets.get(&path) {
                        if checked {
                            widget.add_css_class("folder-checked");
                            play_selection_animation(widget);
                        } else {
                            widget.remove_css_class("folder-checked");
                        }
                    }
                    drop(state);
                    this.notify_selection_changed();
                } else {
                    // Toggle file selection
                    let mut state = this.state.borrow_mut();
                    let selected = state.selection.toggle_file(&path);
                    drop(state);

                    let state = this.state.borrow();
                    if let Some(widget) = state.list_item_widgets.get(&path) {
                        if selected {
                            widget.add_css_class("accent-selected");
                            play_selection_animation(widget);
                        } else {
                            widget.remove_css_class("accent-selected");
                        }
                    }
                    drop(state);
                    this.notify_selection_changed();
                }
            }
        ));
        list_view.add_controller(list_gesture);

        // Single left-click folder navigation (right-click selection is handled by GestureClick)
        list_view.connect_activate(clone!(
            #[weak(rename_to = this)]
            self,
            move |_list_view, position| {
                let item_info: Option<(bool, PathBuf)> = {
                    let model_opt = this.list_model.borrow();
                    if let Some(model) = model_opt.as_ref() {
                        model.item(position).map(|item| {
                            let browser_item = item.downcast::<BrowserItem>().unwrap();
                            let data = browser_item.data();
                            (data.is_folder(), data.path().to_path_buf())
                        })
                    } else {
                        None
                    }
                };

                let Some((is_folder, path)) = item_info else {
                    return;
                };

                if is_folder {
                    let cb = this.state.borrow().on_folder_activated.clone();
                    if let Some(cb) = cb {
                        cb(&path);
                    }
                }
            }
        ));

        let list_scrolled = gtk::ScrolledWindow::new();
        list_scrolled.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        list_scrolled.set_child(Some(&list_view));
        stack.add_named(&list_scrolled, Some("list"));

        // Default to grid view
        stack.set_visible_child_name("grid");

        // Store references
        *self.container.borrow_mut() = Some(vbox.clone());
        *self.grid_view.borrow_mut() = Some(grid_view);
        *self.list_view.borrow_mut() = Some(list_view);
        *self.grid_model.borrow_mut() = Some(grid_model);
        *self.list_model.borrow_mut() = Some(list_model);
        *self.grid_selection.borrow_mut() = Some(grid_no_selection);
        *self.list_selection.borrow_mut() = Some(list_no_selection);
        *self.stack.borrow_mut() = Some(stack.clone());

        // ── Drag & Drop ──────────────────────────────────────────────────
        {
            let drop_target =
                gtk::DropTarget::new(gio::File::static_type(), gtk::gdk::DragAction::COPY);

            let stack_for_enter = stack.clone();
            drop_target.connect_enter(move |_, _, _| {
                stack_for_enter.add_css_class("drop-zone-active");
                gtk::gdk::DragAction::COPY
            });

            let stack_for_leave = stack.clone();
            drop_target.connect_leave(move |_| {
                stack_for_leave.remove_css_class("drop-zone-active");
            });

            let stack_for_drop = stack.clone();
            drop_target.connect_drop(clone!(
                #[weak(rename_to = this)]
                self,
                #[upgrade_or]
                false,
                move |_, value, _, _| {
                    stack_for_drop.remove_css_class("drop-zone-active");

                    if let Ok(file) = value.get::<gio::File>() {
                        if let Some(path) = file.path() {
                            let path = path.to_path_buf();
                            let is_dir = path.is_dir();
                            let target_path = if is_dir {
                                path.clone()
                            } else {
                                path.parent()
                                    .map(|p| p.to_path_buf())
                                    .unwrap_or(path.clone())
                            };

                            // Notify callback
                            let cb = this.state.borrow().on_files_dropped.clone();
                            if let Some(cb) = cb {
                                cb(&target_path);
                            }
                            return true;
                        }
                    }
                    false
                }
            ));

            stack.add_controller(drop_target);
        }

        vbox.append(&stack);
        vbox.set_parent(obj);
    }

    /// Load a thumbnail in a background thread with disk caching.
    ///
    /// Flow:
    ///   1. Background thread: try disk cache → decode with Pixbuf → save to cache
    ///   2. Main thread: create Texture from PNG bytes → update cache and widget
    ///
    /// This is much faster than the previous main-thread approach because:
    ///   - Thumbnails load in parallel on background threads (no main thread blocking)
    ///   - Disk cache avoids re-decoding images on revisits
    ///   - In-memory cache survives directory navigation
    fn load_thumbnail_async(&self, path: &PathBuf) {
        let mut state = self.state.borrow_mut();
        if state.thumbnails.is_loading(path) || state.thumbnails.get(path).is_some() {
            return;
        }
        state.thumbnails.set_loading(path, true);
        drop(state);

        let path_clone = path.clone();
        let path_for_thread = path_clone.clone();
        let obj = self.obj().downgrade();
        let (tx, rx) = async_channel::bounded::<Option<Vec<u8>>>(1);

        // Background thread: I/O-heavy thumbnail loading (disk cache + image decode).
        // Each thumbnail gets its own thread — they all run in parallel.
        std::thread::spawn(move || {
            let result = load_cached_thumbnail(&path_for_thread).or_else(|| {
                // Cache miss — decode image and save to disk cache
                let pixbuf =
                    gdk_pixbuf::Pixbuf::from_file_at_size(&path_for_thread, 112, 112).ok()?;
                let png_bytes = pixbuf.save_to_bufferv("png", &[]).ok()?;
                save_cached_thumbnail(&path_for_thread, &png_bytes);
                Some(png_bytes)
            });
            let _ = tx.send_blocking(result);
        });

        // Main thread: create Texture from PNG bytes and update UI
        glib::spawn_future_local(async move {
            let png_bytes = rx.recv().await.ok().flatten();
            let Some(obj) = obj.upgrade() else {
                return;
            };
            let priv_ = imp(&obj);

            match png_bytes {
                Some(bytes) => {
                    let gbytes = glib::Bytes::from_owned(bytes);
                    let texture = match gtk::gdk::Texture::from_bytes(&gbytes) {
                        Ok(t) => t,
                        Err(e) => {
                            log::debug!("Texture::from_bytes failed for {:?}: {}", path_clone, e);
                            let mut state = priv_.state.borrow_mut();
                            state.thumbnails.set_loading(&path_clone, false);
                            return;
                        }
                    };

                    let mut state = priv_.state.borrow_mut();
                    state.thumbnails.insert(path_clone.clone(), texture.clone());
                    state.thumbnails.set_loading(&path_clone, false);

                    // Update the grid widget if it exists
                    if let Some(widget) = state.grid_item_widgets.get(&path_clone) {
                        if let Some(box_w) = widget.clone().downcast::<gtk::Box>().ok() {
                            if let Some(image) = box_w.first_child() {
                                if let Ok(img) = image.downcast::<gtk::Image>() {
                                    img.set_paintable(Some(&texture));
                                    img.set_pixel_size(112);
                                }
                            }
                            box_w.remove_css_class("thumbnail-loading");
                        }
                    }

                    // Update the list widget if it exists
                    if let Some(widget) = state.list_item_widgets.get(&path_clone) {
                        if let Some(row) = widget.clone().downcast::<gtk::Box>().ok() {
                            if let Some(image) = row.first_child() {
                                if let Ok(img) = image.downcast::<gtk::Image>() {
                                    img.set_paintable(Some(&texture));
                                    img.set_pixel_size(112);
                                }
                            }
                            row.remove_css_class("thumbnail-loading");
                        }
                    }
                }
                None => {
                    log::debug!("Thumbnail load failed for {:?}", path_clone);
                    let mut state = priv_.state.borrow_mut();
                    state.thumbnails.set_loading(&path_clone, false);

                    // Remove shimmer from grid and list widgets
                    let grid_widget = state.grid_item_widgets.get(&path_clone).cloned();
                    let list_widget = state.list_item_widgets.get(&path_clone).cloned();
                    drop(state);

                    if let Some(widget) = grid_widget {
                        if let Some(box_w) = widget.downcast::<gtk::Box>().ok() {
                            box_w.remove_css_class("thumbnail-loading");
                        }
                    }
                    if let Some(widget) = list_widget {
                        if let Some(row) = widget.downcast::<gtk::Box>().ok() {
                            row.remove_css_class("thumbnail-loading");
                        }
                    }
                }
            }
        });
    }

    /// Notify the parent that the selection has changed and update count label.
    fn notify_selection_changed(&self) {
        // Clone the Rc callback so we can drop the borrow before calling.
        // The callback may trigger operations that re-borrow state.
        let cb = self.state.borrow().on_selection_changed.clone();
        let selection = self.state.borrow().selection.clone();
        if let Some(cb) = cb {
            cb(&selection);
        }

        // Update count label
        let state = self.state.borrow();
        let count = state.selection.selected_count();
        let total = state.items.len();
        drop(state);

        if let Some(label) = self.count_label.borrow().as_ref() {
            let new_text = if count > 0 {
                format!("{} {}", count, i18n::t("ausgewählt"))
            } else {
                format!("{} {}", total, i18n::t("Dateien"))
            };

            // Bounce animation if text changed
            let old_text = label.label();
            if old_text != new_text {
                label.remove_css_class("dim-label");
                label.set_label(&new_text);
                if count == 0 {
                    label.add_css_class("dim-label");
                }
                // Trigger bounce — use a 50ms timeout to guarantee the CSS
                // engine has processed the class removal before re-adding it.
                // (idle_add_local_once can fire within the same frame, which
                // makes the animation restart silently fail.)
                label.remove_css_class("count-bounce");
                let label_clone = label.clone();
                glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
                    label_clone.add_css_class("count-bounce");
                    // Auto-remove after animation completes so the class is
                    // always absent before the next trigger.
                    let lc = label_clone.clone();
                    glib::timeout_add_local(std::time::Duration::from_millis(400), move || {
                        lc.remove_css_class("count-bounce");
                        glib::ControlFlow::Break
                    });
                    glib::ControlFlow::Break
                });
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

impl FileBrowser {
    /// Create a new FileBrowser widget.
    pub fn new() -> Self {
        glib::Object::builder().build()
    }

    /// Set the directory to browse and populate the model.
    pub fn set_directory(&self, directory: &Path) {
        log::debug!("set_directory({})", directory.display());
        let priv_ = imp(self);
        let mut state = priv_.state.borrow_mut();
        // Determine navigation direction
        if let Some(parent) = state.current_directory.parent() {
            state.nav_forward = directory.starts_with(parent) && directory != parent;
        }

        state.current_directory = directory.to_path_buf();
        state.items.clear();
        state.all_image_paths.clear();
        state.selection.deselect_all();
        state.thumbnails.clear_loading();
        state.grid_item_widgets.clear();
        state.list_item_widgets.clear();
        drop(state);

        self.refresh();
    }

    /// Refresh the file list from the current directory.
    ///
    /// Directory scanning runs on a background thread to keep the UI
    /// responsive. Results are posted back to the main thread via an
    /// async channel, so the main loop stays responsive during I/O.
    pub fn refresh(&self) {
        let priv_ = imp(self);
        let directory = priv_.state.borrow().current_directory.clone();
        if directory.as_os_str().is_empty() {
            log::warn!("refresh(): current_directory is empty, skipping");
            return;
        }
        log::debug!("refresh({})", directory.display());

        // ═══════════════════════════════════════════════════════════════════
        // Cache check — instant re-display for previously visited dirs
        // ═══════════════════════════════════════════════════════════════════
        {
            let state = priv_.state.borrow();
            if let Some(cached) = state.dir_cache.get(&directory) {
                let mut items = cached.items.clone();
                let all_image_paths = cached.all_image_paths.clone();
                let sort_method = state.sort_method;
                let search = state.search_text.clone();
                drop(state);

                self.sort_items(&mut items, sort_method);
                let filtered: Vec<BrowserItemType> = if search.is_empty() {
                    items
                } else {
                    let search_lower = search.to_lowercase();
                    items
                        .into_iter()
                        .filter(|item| item.display_name().to_lowercase().contains(&search_lower))
                        .collect()
                };

                let mut state = priv_.state.borrow_mut();
                state.items = filtered.clone();
                state.all_image_paths = all_image_paths;
                drop(state);

                let grid_items: Vec<BrowserItem> = filtered
                    .iter()
                    .map(|d| BrowserItem::new(d.clone()))
                    .collect();
                let list_items = grid_items.clone();

                if let Some(model) = priv_.grid_model.borrow().as_ref() {
                    model.remove_all();
                    for item in &grid_items {
                        model.append(item);
                    }
                }
                if let Some(model) = priv_.list_model.borrow().as_ref() {
                    model.remove_all();
                    for item in &list_items {
                        model.append(item);
                    }
                }

                priv_.notify_selection_changed();
                log::debug!("refresh: cache hit for {}", directory.display());
                return;
            }
        }

        // ═══════════════════════════════════════════════════════════════════
        // Phase 1: Instant display — scan current directory only.
        // Folders appear with image_count=0. Image counts are loaded in
        // Phase 2 (parallel, using std::thread::scope) so the UI shows
        // content immediately.  Inspired by yazi's chunk-based loading.
        // ═══════════════════════════════════════════════════════════════════
        type Phase1Result = (Vec<BrowserItemType>, Vec<PathBuf>, Vec<PathBuf>);
        let (tx1, rx1) = async_channel::bounded::<Phase1Result>(1);
        let dir_clone = directory.clone();

        std::thread::spawn(move || {
            let exts = ["png", "jpg", "jpeg", "bmp", "tiff", "tif", "webp", "gif"];
            let mut items: Vec<BrowserItemType> = Vec::new();
            let mut all_image_paths: Vec<PathBuf> = Vec::new();
            let mut folder_paths: Vec<PathBuf> = Vec::new();

            // Single read_dir of the current directory — no subdirectory scanning
            if let Ok(read_dir) = std::fs::read_dir(&dir_clone) {
                for entry in read_dir.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        let name = path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("?")
                            .to_string();
                        folder_paths.push(path.clone());
                        // image_count deferred to Phase 2
                        items.push(BrowserItemType::Folder {
                            path,
                            name,
                            image_count: 0,
                        });
                    } else if path.is_file() {
                        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                            if exts.contains(&ext.to_lowercase().as_str()) {
                                let file_name = path
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("?")
                                    .to_string();
                                let file_size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                                let modified = entry
                                    .metadata()
                                    .ok()
                                    .and_then(|m| m.modified().ok())
                                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                                    .map(|d| d.as_secs());
                                all_image_paths.push(path.clone());
                                items.push(BrowserItemType::Image {
                                    path,
                                    name: file_name,
                                    size_bytes: file_size,
                                    modified,
                                });
                            }
                        }
                    }
                }
            }

            let _ = tx1.send_blocking((items, all_image_paths, folder_paths));
        });

        // ── Phase 1 main-thread handler: instant display ────────────
        let weak = self.downgrade();
        glib::spawn_future_local(async move {
            if let Ok((mut items, all_image_paths, folder_paths)) = rx1.recv().await {
                if let Some(obj) = weak.upgrade() {
                    let priv_ = imp(&obj);

                    // Sort items (fast, on main thread)
                    let sort_method = priv_.state.borrow().sort_method;
                    obj.sort_items(&mut items, sort_method);

                    let folder_count = items.iter().filter(|i| i.is_folder()).count();
                    let image_count = items.iter().filter(|i| i.is_image()).count();

                    // Apply search filter
                    let search = priv_.state.borrow().search_text.clone();
                    let search_was_empty = search.is_empty();
                    let filtered: Vec<BrowserItemType> = if search.is_empty() {
                        items
                    } else {
                        let search_lower = search.to_lowercase();
                        items
                            .into_iter()
                            .filter(|item| {
                                item.display_name().to_lowercase().contains(&search_lower)
                            })
                            .collect()
                    };

                    log::debug!(
                        "refresh phase1: {} folders + {} images ({} after filter)",
                        folder_count,
                        image_count,
                        filtered.len()
                    );

                    // Update state
                    let mut state = priv_.state.borrow_mut();
                    state.items = filtered.clone();
                    state.all_image_paths = all_image_paths;
                    drop(state);

                    // Populate models
                    let grid_items: Vec<BrowserItem> = filtered
                        .iter()
                        .map(|d| BrowserItem::new(d.clone()))
                        .collect();
                    let list_items = grid_items.clone();

                    if let Some(model) = priv_.grid_model.borrow().as_ref() {
                        model.remove_all();
                        for item in &grid_items {
                            model.append(item);
                        }
                    }
                    if let Some(model) = priv_.list_model.borrow().as_ref() {
                        model.remove_all();
                        for item in &list_items {
                            model.append(item);
                        }
                    }

                    priv_.notify_selection_changed();

                    // Capture for Phase 2 cache decision (bool is Copy)
                    let cache_on_complete = search_was_empty;

                    // ═══════════════════════════════════════════════════════
                    // Phase 2: Parallel image counting via std::thread::scope
                    // ═══════════════════════════════════════════════════════
                    // Each subdirectory gets its own thread — all run
                    // concurrently. Results are collected and the model is
                    // rebuilt once with the real image counts.
                    if !folder_paths.is_empty() {
                        let (tx2, rx2) = async_channel::bounded::<Vec<(PathBuf, usize)>>(1);
                        std::thread::spawn(move || {
                            let counts: Vec<(PathBuf, usize)> = std::thread::scope(|s| {
                                let handles: Vec<_> = folder_paths
                                    .into_iter()
                                    .map(|p| {
                                        s.spawn(move || {
                                            let exts = [
                                                "png", "jpg", "jpeg", "bmp", "tiff", "tif", "webp",
                                                "gif",
                                            ];
                                            let count = std::fs::read_dir(&p)
                                                .ok()
                                                .map(|rd| {
                                                    rd.flatten()
                                                        .filter(|e| {
                                                            e.path()
                                                                .extension()
                                                                .and_then(|ext| ext.to_str())
                                                                .map(|ext| {
                                                                    exts.contains(
                                                                        &ext.to_lowercase()
                                                                            .as_str(),
                                                                    )
                                                                })
                                                                .unwrap_or(false)
                                                        })
                                                        .count()
                                                })
                                                .unwrap_or(0);
                                            (p, count)
                                        })
                                    })
                                    .collect();
                                handles.into_iter().map(|h| h.join().unwrap()).collect()
                            });
                            let _ = tx2.send_blocking(counts);
                        });

                        let weak2 = obj.downgrade();
                        glib::spawn_future_local(async move {
                            if let Ok(counts) = rx2.recv().await {
                                if let Some(obj) = weak2.upgrade() {
                                    let priv_ = imp(&obj);
                                    let count_map: HashMap<PathBuf, usize> =
                                        counts.into_iter().collect();

                                    // Update folder items with real image counts
                                    let mut state = priv_.state.borrow_mut();
                                    let mut updated = false;
                                    for item in &mut state.items {
                                        if let BrowserItemType::Folder {
                                            path, image_count, ..
                                        } = item
                                        {
                                            if let Some(&c) = count_map.get(path) {
                                                if *image_count != c {
                                                    *image_count = c;
                                                    updated = true;
                                                }
                                            }
                                        }
                                    }

                                    // Cache fully resolved items (with real image counts)
                                    // for instant re-display on next visit.
                                    if cache_on_complete {
                                        let dir = state.current_directory.clone();
                                        let cached_items = state.items.clone();
                                        let cached_paths = state.all_image_paths.clone();
                                        state.dir_cache.insert(
                                            dir,
                                            DirCacheEntry {
                                                items: cached_items,
                                                all_image_paths: cached_paths,
                                            },
                                        );
                                        // Evict oldest entries if cache grows too large
                                        while state.dir_cache.len() > 64 {
                                            if let Some(k) = state.dir_cache.keys().next().cloned()
                                            {
                                                state.dir_cache.remove(&k);
                                            }
                                        }
                                        log::debug!(
                                            "refresh: cached dir ({} items)",
                                            state.items.len()
                                        );
                                    }

                                    if updated {
                                        let filtered = state.items.clone();
                                        drop(state);

                                        let grid_items: Vec<BrowserItem> = filtered
                                            .iter()
                                            .map(|d| BrowserItem::new(d.clone()))
                                            .collect();
                                        let list_items = grid_items.clone();

                                        if let Some(model) = priv_.grid_model.borrow().as_ref() {
                                            model.remove_all();
                                            for item in &grid_items {
                                                model.append(item);
                                            }
                                        }
                                        if let Some(model) = priv_.list_model.borrow().as_ref() {
                                            model.remove_all();
                                            for item in &list_items {
                                                model.append(item);
                                            }
                                        }
                                    }
                                }
                            }
                        });
                    }
                }
            }
        });
    }

    /// Force-refresh: invalidate cache for current directory and rescan.
    /// Called by the F5 / Ctrl+R shortcut so the user can manually refresh.
    pub fn force_refresh(&self) {
        let priv_ = imp(self);
        let directory = priv_.state.borrow().current_directory.clone();
        if !directory.as_os_str().is_empty() {
            priv_.state.borrow_mut().dir_cache.remove(&directory);
            log::debug!(
                "force_refresh: invalidated cache for {}",
                directory.display()
            );
        }
        self.refresh();
    }

    /// Sort items by the given method.
    fn sort_items(&self, items: &mut [BrowserItemType], method: u32) {
        match method {
            0 => {
                // Name (A-Z)
                items.sort_by(|a, b| {
                    a.display_name()
                        .to_lowercase()
                        .cmp(&b.display_name().to_lowercase())
                });
            }
            1 => {
                // Natural sort (1, 2, 10, 11, ...)
                items.sort_by(|a, b| {
                    alphanumeric_sort::compare_str(a.display_name(), b.display_name())
                });
            }
            2 => {
                // Date (newest first) — folders first, then files by date
                items.sort_by(|a, b| {
                    // Folders always come first
                    match (a.is_folder(), b.is_folder()) {
                        (true, false) => std::cmp::Ordering::Less,
                        (false, true) => std::cmp::Ordering::Greater,
                        _ => {
                            let a_time = match a {
                                BrowserItemType::Image { modified, .. } => *modified,
                                _ => None,
                            };
                            let b_time = match b {
                                BrowserItemType::Image { modified, .. } => *modified,
                                _ => None,
                            };
                            b_time.cmp(&a_time) // newest first
                        }
                    }
                });
            }
            _ => {}
        }

        // Folders always before files (regardless of sort method)
        items.sort_by(|a, b| match (a.is_folder(), b.is_folder()) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => std::cmp::Ordering::Equal,
        });
    }

    /// Switch between grid and list view.
    pub fn set_view_mode(&self, mode: &str) {
        let priv_ = imp(self);
        priv_.state.borrow_mut().view_mode = mode.to_string();

        if let Some(stack) = priv_.stack.borrow().as_ref() {
            match mode {
                "list" => stack.set_visible_child_name("list"),
                _ => stack.set_visible_child_name("grid"),
            }
        }
    }

    /// Set the search filter text and refresh.
    pub fn set_search(&self, text: &str) {
        let priv_ = imp(self);
        priv_.state.borrow_mut().search_text = text.to_string();
        self.refresh();
    }

    /// Set the sort method index.
    pub fn set_sort_method(&self, method: u32) {
        let priv_ = imp(self);
        priv_.state.borrow_mut().sort_method = method;
        self.refresh();
    }

    /// Select all visible files.
    pub fn select_all(&self) {
        let priv_ = imp(self);
        let all_paths = priv_.state.borrow().all_image_paths.clone();
        let mut state = priv_.state.borrow_mut();
        state.selection.select_all(&all_paths);

        // Apply CSS to all visible grid items
        for (path, widget) in &state.grid_item_widgets {
            if state.selection.is_file_selected(path) {
                widget.add_css_class("accent-selected");
            }
        }
        for (path, widget) in &state.list_item_widgets {
            if state.selection.is_file_selected(path) {
                widget.add_css_class("accent-selected");
            }
        }
        drop(state);
        priv_.notify_selection_changed();
    }

    /// Deselect all files and folders.
    pub fn deselect_all(&self) {
        let priv_ = imp(self);

        // Remove CSS from all widgets before clearing state
        {
            let state = priv_.state.borrow();
            for widget in state.grid_item_widgets.values() {
                widget.remove_css_class("accent-selected");
                widget.remove_css_class("folder-checked");
            }
            for widget in state.list_item_widgets.values() {
                widget.remove_css_class("accent-selected");
                widget.remove_css_class("folder-checked");
            }
        }

        priv_.state.borrow_mut().selection.deselect_all();
        priv_.notify_selection_changed();
    }

    /// Get all currently selected file paths.
    pub fn selected_files(&self) -> Vec<PathBuf> {
        let priv_ = imp(self);
        priv_.state.borrow().selection.all_selected_files()
    }

    /// Get the current selection state (read-only reference via clone).
    pub fn selection(&self) -> SelectionState {
        let priv_ = imp(self);
        priv_.state.borrow().selection.clone()
    }

    /// Set a callback for selection changes.
    pub fn on_selection_changed<F: Fn(&SelectionState) + 'static>(&self, callback: F) {
        let priv_ = imp(self);
        priv_.state.borrow_mut().on_selection_changed = Some(Rc::new(callback));
    }

    /// Set a callback for folder activation (double-click / enter).
    pub fn on_folder_activated<F: Fn(&Path) + 'static>(&self, callback: F) {
        let priv_ = imp(self);
        priv_.state.borrow_mut().on_folder_activated = Some(Rc::new(callback));
    }

    /// Set a callback for drag & drop (files or folders dropped onto browser).
    pub fn on_files_dropped<F: Fn(&Path) + 'static>(&self, callback: F) {
        let priv_ = imp(self);
        priv_.state.borrow_mut().on_files_dropped = Some(Rc::new(callback));
    }

    /// Get the current directory.
    pub fn current_directory(&self) -> PathBuf {
        let priv_ = imp(self);
        priv_.state.borrow().current_directory.clone()
    }

    /// Get total item count.
    pub fn item_count(&self) -> usize {
        let priv_ = imp(self);
        priv_.state.borrow().items.len()
    }

    /// Get total image count (excluding folders).
    pub fn image_count(&self) -> usize {
        let priv_ = imp(self);
        priv_
            .state
            .borrow()
            .items
            .iter()
            .filter(|i| i.is_image())
            .count()
    }

    /// Set the Python bridge instance.
    pub fn set_bridge(&self, bridge: Arc<IpcBridge>) {
        let priv_ = imp(self);
        priv_.state.borrow_mut().bridge = bridge;
    }
}

// ---------------------------------------------------------------------------
// BrowserItem — GObject wrapper for items in the Gio.ListModel
// ---------------------------------------------------------------------------

glib::wrapper! {
    pub struct BrowserItem(ObjectSubclass<BrowserItemPrivate>)
        @implements gio::ListModel;
}

impl BrowserItem {
    pub fn new(data: BrowserItemType) -> Self {
        let obj = glib::Object::builder::<Self>().build();
        obj.imp().data.replace(Some(data));
        obj
    }

    pub fn data(&self) -> BrowserItemType {
        self.imp()
            .data
            .borrow()
            .clone()
            .unwrap_or_else(|| BrowserItemType::Folder {
                path: PathBuf::new(),
                name: String::new(),
                image_count: 0,
            })
    }
}

pub struct BrowserItemPrivate {
    data: RefCell<Option<BrowserItemType>>,
}

impl Default for BrowserItemPrivate {
    fn default() -> Self {
        Self {
            data: RefCell::new(None),
        }
    }
}

#[glib::object_subclass]
impl ObjectSubclass for BrowserItemPrivate {
    const NAME: &'static str = "MangaBrowserItem";
    type Type = BrowserItem;
    type ParentType = glib::Object;
}

impl ObjectImpl for BrowserItemPrivate {}

// Implement Gio.ListModel interface for BrowserItem
impl gio::subclass::prelude::ListModelImpl for BrowserItemPrivate {
    fn item_type(&self) -> glib::Type {
        BrowserItem::static_type()
    }

    fn n_items(&self) -> u32 {
        0
    }

    fn item(&self, _position: u32) -> Option<glib::Object> {
        None
    }
}

// ---------------------------------------------------------------------------
// Utility functions
// ---------------------------------------------------------------------------

/// Format file size in human-readable form.
fn format_file_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;

    if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.0} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Get the private implementation of a FileBrowser.
fn imp(obj: &FileBrowser) -> &FileBrowserPrivate {
    obj.imp()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_selection_toggle_file() {
        let mut sel = SelectionState::default();
        let path = PathBuf::from("/test/image.png");

        assert!(!sel.is_file_selected(&path));
        let selected = sel.toggle_file(&path);
        assert!(selected);
        assert!(sel.is_file_selected(&path));

        let selected = sel.toggle_file(&path);
        assert!(!selected);
        assert!(!sel.is_file_selected(&path));
    }

    #[test]
    fn test_selection_toggle_folder() {
        let mut sel = SelectionState::default();
        let folder = PathBuf::from("/test/chapter1");
        let images = vec![
            PathBuf::from("/test/chapter1/01.png"),
            PathBuf::from("/test/chapter1/02.png"),
        ];

        assert!(!sel.is_folder_checked(&folder));
        let checked = sel.toggle_folder(&folder, images.clone());
        assert!(checked);
        assert!(sel.is_folder_checked(&folder));

        let checked = sel.toggle_folder(&folder, images);
        assert!(!checked);
        assert!(!sel.is_folder_checked(&folder));
    }

    #[test]
    fn test_select_all() {
        let mut sel = SelectionState::default();
        let paths = vec![
            PathBuf::from("/a.png"),
            PathBuf::from("/b.png"),
            PathBuf::from("/c.png"),
        ];
        sel.select_all(&paths);
        assert_eq!(sel.selected_files.len(), 3);
    }

    #[test]
    fn test_deselect_all() {
        let mut sel = SelectionState::default();
        sel.toggle_file(&PathBuf::from("/a.png"));
        sel.toggle_folder(
            &PathBuf::from("/folder"),
            vec![PathBuf::from("/folder/1.png")],
        );
        sel.deselect_all();
        assert!(sel.selected_files.is_empty());
        assert!(sel.checked_folders.is_empty());
    }

    #[test]
    fn test_all_selected_files() {
        let mut sel = SelectionState::default();
        sel.toggle_file(&PathBuf::from("/a.png"));
        sel.toggle_folder(
            &PathBuf::from("/f"),
            vec![PathBuf::from("/f/1.png"), PathBuf::from("/f/2.png")],
        );
        let all = sel.all_selected_files();
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn test_format_file_size() {
        assert_eq!(format_file_size(500), "500 B");
        assert_eq!(format_file_size(1500), "1 KB");
        assert_eq!(format_file_size(2_500_000), "2.4 MB");
    }

    #[test]
    fn test_browser_item_type() {
        let folder = BrowserItemType::Folder {
            path: PathBuf::from("/test"),
            name: "test".into(),
            image_count: 5,
        };
        assert!(folder.is_folder());
        assert!(!folder.is_image());
        assert_eq!(folder.display_name(), "test");

        let image = BrowserItemType::Image {
            path: PathBuf::from("/test/01.png"),
            name: "01.png".into(),
            size_bytes: 1024,
            modified: Some(123456),
        };
        assert!(!image.is_folder());
        assert!(image.is_image());
        assert_eq!(image.display_name(), "01.png");
    }

    #[test]
    fn test_thumbnail_cache() {
        let mut cache = ThumbnailCache::default();
        let path = PathBuf::from("/test.png");
        assert!(cache.get(&path).is_none());
        assert!(!cache.is_loading(&path));

        cache.set_loading(&path, true);
        assert!(cache.is_loading(&path));
        cache.set_loading(&path, false);
        assert!(!cache.is_loading(&path));
    }
}
