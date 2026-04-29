use gtk::gio;
use gtk::prelude::*;
use gtk::{
    Application, ApplicationWindow, Box as GtkBox, CssProvider, Paned,
    MenuButton, Orientation, STYLE_PROVIDER_PRIORITY_APPLICATION, ScrolledWindow, gdk, glib, Notebook
};
use sourceview5 as sv;
use sourceview5::prelude::*;
use std::fs;
use vte4 as vte;
use vte::prelude::*;
use gtk::{Settings};
use std::process::Command;
fn set_caret_style() {
    let display = gdk::Display::default().expect("No display");
    let settings = Settings::for_display(&display);

    // Make the caret thicker than the default 0.04
    settings.set_gtk_cursor_aspect_ratio(0.10);

    // Optional blink tuning
    settings.set_gtk_cursor_blink(true);
    settings.set_gtk_cursor_blink_time(800);
    // settings.set_gtk_cursor_blink_timeout(0);
}
static TAB_WIDTH: &str = "    ";
const APP_ID: &str = "nerd.ide.gtk4rs";

use sourceview5::{Buffer, View};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;
use std::thread;
thread_local! {
    static VARS_PROVIDER: RefCell<Option<CssProvider>> = const { RefCell::new(None) };
}
use std::cell::{Cell};
// use sourceview5::ffi::gtk_source_print_compositor_get_line_numbers_font_name;
// use gtk::AccessibleRole::Command;

thread_local! {
    static TERMINAL: Cell<bool> = const { Cell::new(false) };
    static TERMINAL2: Cell<bool> = const { Cell::new(false) };
    static SCHEME: RefCell<Option<sv::StyleScheme>> = const { RefCell::new(None) };
    static BUFFERS: RefCell<Vec<Buffer>> = const { RefCell::new(Vec::new()) };
}
fn load_css() {
    let static_provider = CssProvider::new();
    static_provider.load_from_path("src/style.css");

    let vars_provider = CssProvider::new();
    vars_provider.load_from_string(":root { --bg: #1e1e2e; --text: #cdd6f4; --btnbg: #1e1e2e; --fg: #45475a; }");

    let display = gdk::Display::default().expect("Could not connect to a display");

    gtk::style_context_add_provider_for_display(
        &display,
        &static_provider,
        STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    gtk::style_context_add_provider_for_display(
        &display,
        &vars_provider,
        STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    VARS_PROVIDER.with(|slot| {
        *slot.borrow_mut() = Some(vars_provider);
    });
}

fn update_css(color: &str, text: &str, fg:&str) {
    let contents = format!(":root {{ --bg: {color}; --text: {text}; --btnbg: {color}; --fg: {fg};", color=color, text=text, fg=fg);
    println!("{}", contents);
    VARS_PROVIDER.with(|slot| {
        if let Some(provider) = slot.borrow().as_ref() {
            provider.load_from_string(&contents);
        }
    });
}

use std::{
    sync::mpsc,
};

use glib::ControlFlow;
use gtk::prelude::*;

fn underline_error(buffer: &Buffer, path: String) {
    let (tx, rx) = mpsc::channel::<String>();

    // Run g++ in the background.
    thread::spawn(move || {
        let stderr = Command::new("g++")
            .arg(&path)
            .arg("-fsyntax-only")
            .arg("-fmessage-length=0")
            .arg("-fno-diagnostics-show-option")
            .output()
            .map(|out| String::from_utf8_lossy(&out.stderr).to_string())
            .unwrap_or_else(|err| {
                eprintln!("failed to run g++: {err}");
                String::new()
            });

        let _ = tx.send(stderr);
    });

    // Keep GTK work on the main thread.
    let buffer = buffer.clone();

    glib::timeout_add_local(Duration::from_millis(30), move || {
        match rx.try_recv() {
            Ok(stderr) => {
                apply_error_underlines(&buffer, &stderr);
                ControlFlow::Break
            }

            Err(mpsc::TryRecvError::Empty) => {
                // g++ is still running; keep checking.
                ControlFlow::Continue
            }

            Err(mpsc::TryRecvError::Disconnected) => {
                ControlFlow::Break
            }
        }
    });
}

fn apply_error_underlines(buffer: &Buffer, stderr: &str) {
    let tag = if let Some(tag) = buffer.tag_table().lookup("error-underline") {
        tag
    } else {
        let tag = buffer
            .create_tag(Some("error-underline"), &[])
            .expect("failed to create tag");

        tag.set_underline(pango::Underline::Error);
        tag.set_underline_rgba(Some(&gdk::RGBA::new(1.0, 0.0, 0.0, 1.0)));
        tag.set_background_rgba(Some(&gdk::RGBA::new(1.0, 0.0, 0.0, 0.18)));

        tag
    };

    let (start, end) = buffer.bounds();
    buffer.remove_tag_by_name("error-underline", &start, &end);

    for line in stderr.lines() {
        // Usually: file.cpp:12:4: error: ...
        if let Some(first) = line.find(':') {
            if let Some(second) = line[first + 1..].find(':') {
                let line_number_text = &line[first + 1..first + second + 1];

                let code_line = line_number_text.parse::<i32>().unwrap_or(1);

                if let Some(line_start) = buffer.iter_at_line(code_line - 1) {
                    let mut line_end = line_start;
                    line_end.forward_to_line_end();

                    buffer.apply_tag(&tag, &line_start, &line_end);
                }
            }
        }
    }
}
fn install_autosave(buffer: &Buffer, path: String) {
    let pending_save: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));

    let path = Rc::new(path);
    let buffer_clone = buffer.clone();
    let pending_save_clone = pending_save.clone();

    buffer.connect_changed(move |_| {
        // cancel previous scheduled save
        if let Some(id) = pending_save_clone.borrow_mut().take() {
            id.remove();
        }
        let buffer_for_save = buffer_clone.clone();
        let path_for_save = path.clone();
        let pending_save_for_save = pending_save_clone.clone();
        let path2 = path.clone();
        let id = glib::timeout_add_local_once(Duration::from_millis(0), move || {
            let (start, end) = buffer_for_save.bounds();
            let text = buffer_for_save.text(&start, &end, true);
            match fs::write(path_for_save.as_str(), text.as_str()) {
                Ok(()) => {
                    buffer_for_save.set_modified(false);
                    println!("autosaved");
                }
                Err(err) => {
                    eprintln!("autosave failed: {err}");
                }
            }
            let lm = sv::LanguageManager::default();
            if let Some(lang) = lm.guess_language(Some(path2.clone().as_str()), None) {
                if lang.to_string() == "C++" {
                    underline_error(&buffer_for_save, path2.clone().as_str().to_string());
                }
            }

            *pending_save_for_save.borrow_mut() = None;
        });

        *pending_save_clone.borrow_mut() = Some(id);
    });
}

fn install_br(view: &View, buffer: &Buffer) {
    let key = gtk::EventControllerKey::new();
    key.set_propagation_phase(gtk::PropagationPhase::Capture);
    let buffer = buffer.clone();
    key.connect_key_pressed(move |_, key, _keycode, state| {
        if state.contains(gdk::ModifierType::CONTROL_MASK)
            || state.contains(gdk::ModifierType::ALT_MASK)
        {
            return glib::Propagation::Proceed;
        }
        let ch = match key {
            gdk::Key::parenleft => "(",
            gdk::Key::parenright => ")",
            gdk::Key::bracketleft => "[",
            gdk::Key::bracketright => "]",
            gdk::Key::braceleft => "{",
            gdk::Key::braceright => "}",
            gdk::Key::quotedbl => "\"",
            gdk::Key::apostrophe => "'",
            gdk::Key::BackSpace => "-",
            _ => return glib::Propagation::Proceed
        };
        let closing = match ch {
            "(" => Some(")"),
            "[" => Some("]"),
            "{" => Some("}"),
            "\"" => Some("\""),
            "'" => Some("'"),
            _ => None,
        };
        if ch != ")" && ch != "]" && ch != "}" && ch != "-" {
            if let Some(close) = closing {
                if let Some(mark) = buffer.mark("insert") {
                    let mut iter = buffer.iter_at_mark(&mark);
                    buffer.begin_user_action();
                    buffer.insert(&mut iter, ch);
                    buffer.insert(&mut iter, close);
                    iter.backward_char();
                    buffer.place_cursor(&iter);
                    buffer.end_user_action();
                    return glib::Propagation::Stop;
                }
                return glib::Propagation::Proceed;
            }
        }
        let insert_mark = buffer.get_insert();
        let start = buffer.iter_at_mark(&insert_mark);
        let mut end = start;
        end.forward_char();
        let next = buffer.text(&start, &end, false);
        if next.as_str() == ch {
            if let Some(mark) = buffer.mark("insert") {
                let mut iter = buffer.iter_at_mark(&mark);
                buffer.begin_user_action();
                iter.forward_char();
                println!("I should jump");
                buffer.place_cursor(&iter);
                buffer.end_user_action();
                return glib::Propagation::Stop;
            }
        }
        if ch == "-" {
            if let Some(mark) = buffer.mark("insert") {
                let mut before = buffer.iter_at_mark(&mark);
                buffer.begin_user_action();
                let mut after = before.clone();
                before.backward_char();
                let bch = before.char();
                let ach = after.char();
                if (bch == '(' && ach == ')') || (bch == '{' && ach == '}') || (bch == '{' && ach == '}') {
                    before.forward_char();
                    after.forward_char();
                    buffer.delete(&mut before, &mut after);
                }
                buffer.end_user_action();
            }
        }
        glib::Propagation::Proceed
    });
    println!("I am adding this");
    view.add_controller(key);
}

fn main() -> glib::ExitCode {
    let app = Application::builder().application_id(APP_ID).build();
    app.connect_startup(|_app| {
        load_css();
    });
    app.connect_activate(|app| {
        build_ui(app, false);
    });
    app.run()
}

fn language_formatting(lang: &str, view: &View, buffer: &Buffer) {
    let key = gtk::EventControllerKey::new();
    key.set_propagation_phase(gtk::PropagationPhase::Capture);

    let buffer = buffer.clone();
    println!("{}", lang);
    let lang = lang;
    let lang = lang.to_string();
    key.connect_key_pressed(move |_, key, _keycode, state| {
        if state.contains(gdk::ModifierType::CONTROL_MASK)
            || state.contains(gdk::ModifierType::ALT_MASK)
        {
            return glib::Propagation::Proceed;
        }

        if key != gdk::Key::Return {
            return glib::Propagation::Proceed;
        }

        let insert = buffer.get_insert();
        let mut cursor = buffer.iter_at_mark(&insert);

        let line = cursor.line();
        let line_start = buffer.iter_at_line(line).unwrap_or_else(|| buffer.start_iter());
        let line_text = buffer.text(&line_start, &cursor, false).to_string();

        let base_indent: String = line_text
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect();

        let trimmed = line_text.trim_end();

        let unit = TAB_WIDTH;
        let mut new_indent = base_indent.clone();
        if trimmed.ends_with('{') {
            new_indent.push_str(unit);

            buffer.begin_user_action();
            buffer.insert(&mut cursor, "\n");
            buffer.insert(&mut cursor, "\n");
            println!("{}", cursor.char());
            let mut cursor2 = cursor.clone();
            cursor2.backward_char();
            buffer.insert(&mut cursor, &base_indent);
            cursor.backward_line();
            buffer.insert(&mut cursor, &new_indent);
            buffer.place_cursor(&cursor);
            // cursor.forward_char();
            buffer.end_user_action();
        } else if trimmed.ends_with(':') && lang == "Python"  {
            buffer.insert(&mut cursor, "\n");
            new_indent.push_str(unit);
            buffer.insert(&mut cursor, &new_indent);
        }
        else {
            buffer.insert(&mut cursor, "\n");
            buffer.insert(&mut cursor, &base_indent);
        }
        glib::Propagation::Stop
    });

    view.add_controller(key);
}
fn build_ui(app: &Application, _build_footer: bool) {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("NerdIDE")
        .default_width(1920)
        .default_height(1080)
        .build();

    let notebook = Notebook::new();
    notebook.set_scrollable(true);
    notebook.set_hexpand(true);
    notebook.set_vexpand(true);
    build_formal(&window, notebook);
    window.present();
    set_caret_style();
}

fn toggle_term(updown: Paned, _notebook: Notebook, _parent: GtkBox) -> () {
    if TERMINAL.with(|v| v.get())  {
        let term = vte::Terminal::new();
        term.set_hexpand(true);
        term.set_vexpand(true);
        term.set_scrollback_lines(10_000);
        term.set_scroll_on_output(true);
        term.set_input_enabled(true);
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
        let argv = [shell.as_str()];
        term.spawn_async(
            vte::PtyFlags::DEFAULT,
            Some("~"),
            &argv,
            &[],
            glib::SpawnFlags::DEFAULT,
            || {},
            -1,
            None::<&gio::Cancellable>,
            |result| match result {
                Ok(_) => println!("Shell started"),
                Err(err) => eprintln!("Failed to start shell: {err}"),
            },
        );

        let term_for_focus = term.clone();
        glib::idle_add_local_once(move || {
            term_for_focus.grab_focus();
        });
        term.add_css_class("term");
        println!("Added notebook");
        updown.set_end_child(Some(&term));
    }
    else {
        updown.set_end_child(None::<&gtk::Widget>);
    }
}

// new func

fn toggle_term2(leftright: Paned, _notebook: Notebook, _parent: GtkBox) -> () {
    if TERMINAL2.with(|v| v.get())  {
        let term = vte::Terminal::new();
        term.set_hexpand(true);
        term.set_vexpand(true);
        term.set_scrollback_lines(10_000);
        term.set_scroll_on_output(true);
        term.set_input_enabled(true);
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
        let argv = [shell.as_str()];
        term.spawn_async(
            vte::PtyFlags::DEFAULT,
            Some("~"),
            &argv,
            &[],
            glib::SpawnFlags::DEFAULT,
            || {},
            -1,
            None::<&gio::Cancellable>,
            |result| match result {
                Ok(_) => println!("Shell started"),
                Err(err) => eprintln!("Failed to start shell: {err}"),
            },
        );

        let term_for_focus = term.clone();
        glib::idle_add_local_once(move || {
            term_for_focus.grab_focus();
        });
        term.add_css_class("term");
        println!("Added notebook");
        leftright.set_end_child(Some(&term));
    }
    else {
        leftright.set_end_child(None::<&gtk::Widget>);
    }
}


fn build_window(window: &ApplicationWindow, notebook: Notebook) {
    let header = GtkBox::new(Orientation::Vertical, 10);
    let world = GtkBox::new(Orientation::Horizontal, 6);
    let parent = GtkBox::new(Orientation::Vertical, 8);
    world.add_css_class("world");
    let window0 = window.clone();
    let notebook0 = notebook.clone();
    let notebook4 = notebook.clone();
    world.set_hexpand(true);
    world.set_vexpand(true);
    parent.set_vexpand(true);
    let menu = gio::Menu::new();
    menu.append(Some("Open"), Some("win.open"));
    menu.append(Some("Save as"), Some("win.saveas"));
    menu.append(Some("New File"), Some("win.newfile"));

    let window_clone = window.clone();
    let save_as = gio::SimpleAction::new("saveas", None);
    let newfile = gio::SimpleAction::new("newfile", None);

    newfile.connect_activate(move |_, _| {
        let dialog = gtk::FileDialog::builder()
            .title("New File")
            .modal(true)
            .accept_label("Select")
            .build();

        let window2 = window_clone.clone();
        let notebook2 = notebook.clone();
        dialog.save(
            Some(&window_clone),
            None::<&gio::Cancellable>,
            move |result| {
                let _window3 = window2.clone();
                match result {
                    Ok(file) => {
                        if let Some(path) = file.path() {
                            let path2 = path.clone();
                            let path3 = path.clone();
                            let _ = fs::File::create(&path);
                            let filename: &str;
                            let tab = gtk::Box::new(Orientation::Horizontal, 0);
                            let source_file = sv::File::new();
                            source_file.set_location(Some(&file));
                            let lm = sv::LanguageManager::default();
                            let mut buffer = Buffer::new(None);
                            if let Some(lang) = lm.guess_language(Some(path.clone()), None) {
                                buffer = Buffer::with_language(&lang);
                            }
                            let gio_file = gio::File::for_path(path);
                            let source_file = sv::File::new();
                            source_file.set_location(Some(&gio_file));
                            let loader = sv::FileLoader::new(&buffer, &source_file);
                            loader.load_async(
                                glib::Priority::DEFAULT,
                                None::<&gio::Cancellable>,
                                move |result| match result {
                                    Ok(()) => println!("Loaded"),
                                    Err(err) => eprintln!("Load failed: {err}"),
                                },
                            );
                            let spaces = TAB_WIDTH.chars().filter(|&c| c == ' ').count() as u32;
                            let indent_width = spaces as i32;
                            let view = View::with_buffer(&buffer);
                            view.set_show_line_numbers(true);
                            view.set_highlight_current_line(true);
                            view.set_auto_indent(true);
                            view.set_indent_width(indent_width);
                            view.set_tab_width(spaces);
                            view.set_insert_spaces_instead_of_tabs(true);
                            view.set_indent_on_tab(true);
                            view.set_smart_backspace(true);
                            view.set_monospace(true);
                            if let Some(lang) = lm.guess_language(Some(path2.clone()), None) {
                                language_formatting(lang.to_string().as_str(), &view, &buffer);
                            }
                            install_br(&view, &buffer);
                            SCHEME.with(|cell| {
                                if let Some(scheme) = cell.borrow().as_ref() {
                                    buffer.set_style_scheme(Some(scheme));
                                }
                            });
                            BUFFERS.with(|buffers| {
                                buffers.borrow_mut().push(buffer.clone());
                            });
                            let scrolled = ScrolledWindow::builder()
                                .child(&view)
                                // .min_content_height(300)
                                .build();
                            scrolled.set_vexpand(true);
                            scrolled.set_hexpand(true);
                            install_autosave(&buffer, path3.to_str().unwrap().to_string());
                            let path21 = path3.to_str().unwrap().to_string();
                            let scrolled2 = scrolled.clone();
                            if let Some(index) = path21.rfind('/') {
                                filename = &path21[index + 1..];
                                tab.append(&gtk::Label::new(Some(filename)));
                                let close = gtk::Button::with_label("");
                                let notebook_clone = notebook2.clone();
                                close.connect_clicked(move |_| {
                                    let page = notebook_clone.page_num(&scrolled);
                                    if page != Some(u32::MAX) {
                                        notebook_clone.remove_page(page);
                                    }
                                });
                                tab.append(&close);
                                close.add_css_class("close-button");
                            }
                            notebook2.append_page(&scrolled2, Some(&tab));
                            notebook2.set_tab_reorderable(&scrolled2, true);
                            notebook2.set_tab_detachable(&scrolled2, true);
                            // build_body(&window3, terminal, notebook2);
                        }
                    }
                    Err(err) => {
                        eprintln!("Save dialog canceled or failed: {err}");
                    }
                }
            },
        );
    });

    let wc = window.clone();
    save_as.connect_activate(move |_, _| {
        let dialog = gtk::FileDialog::builder()
            .title("Save As")
            .modal(true)
            .accept_label("Save")
            .build();

        let window2 = wc.clone();

        dialog.save(
            Some(&window2),
            None::<&gio::Cancellable>,
            move |result| match result {
                Ok(file) => {
                    if let Some(path) = file.path() {
                        let path2 = path.clone();
                        let text = fs::read_to_string(path2).unwrap();
                        if let Err(err) = fs::write(&path, text.as_str()) {
                            eprintln!("Failed to save: {err}");
                        }
                    } else {
                        eprintln!("Selected location is not a local path");
                        eprintln!("URI: {}", file.uri());
                    }
                }
                Err(err) => {
                    eprintln!("Save dialog canceled or failed: {err}");
                }
            },
        );
    });

    let open_action = gio::SimpleAction::new("open", None);

    open_action.connect_activate(move |_, _| {
        let dialog = gtk::FileDialog::builder()
            .title("Choose a file")
            .modal(true)
            .build();
        let nt = notebook0.clone();
        let _window_for_dialog = window0.clone();
        dialog.open(
            Some(&window0),
            None::<&gio::Cancellable>,
            move |result| {
                match result {
                    Ok(file) => {
                        if let Some(path) = file.path() {
                            let path2 = path.clone();
                            let path3 = path.clone();
                            let filename: &str;
                            let tab = gtk::Box::new(Orientation::Horizontal, 8);
                            let source_file = sv::File::new();
                            source_file.set_location(Some(&file));
                            let lm = sv::LanguageManager::default();
                            let mut buffer = Buffer::new(None);
                            if let Some(lang) = lm.guess_language(Some(path.clone()), None) {
                                buffer = Buffer::with_language(&lang);
                            }
                            let gio_file = gio::File::for_path(path);
                            let source_file = sv::File::new();
                            source_file.set_location(Some(&gio_file));
                            let loader = sv::FileLoader::new(&buffer, &source_file);
                            loader.load_async(
                                glib::Priority::DEFAULT,
                                None::<&gio::Cancellable>,
                                move |result| match result {
                                    Ok(()) => println!("Loaded"),
                                    Err(err) => eprintln!("Load failed: {err}"),
                                },
                            );
                            let spaces = TAB_WIDTH.chars().filter(|&c| c == ' ').count() as u32;
                            let indent_width = spaces as i32;
                            let view = View::with_buffer(&buffer);
                            view.set_show_line_numbers(true);
                            view.set_highlight_current_line(true);
                            view.set_auto_indent(true);
                            view.set_indent_width(indent_width);
                            view.set_tab_width(spaces);
                            view.set_insert_spaces_instead_of_tabs(true);
                            view.set_indent_on_tab(true);
                            view.set_smart_backspace(true);
                            view.set_monospace(true);
                            if let Some(lang) = lm.guess_language(Some(path2.clone()), None) {
                                language_formatting(lang.to_string().as_str(), &view, &buffer);
                            }
                            install_br(&view, &buffer);
                            SCHEME.with(|cell| {
                                if let Some(scheme) = cell.borrow().as_ref() {
                                    buffer.set_style_scheme(Some(scheme));
                                }
                            });
                            BUFFERS.with(|buffers| {
                                buffers.borrow_mut().push(buffer.clone());
                            });
                            let scrolled = ScrolledWindow::builder()
                                .child(&view)
                                // .min_content_height(300)
                                .build();
                            scrolled.set_vexpand(true);
                            scrolled.set_hexpand(true);
                            let path21 = path3.to_str().unwrap().to_string();
                            let scrolled2 = scrolled.clone();
                            if let Some(index) = path21.rfind('/') {
                                filename = &path21[index + 1..];
                                tab.append(&gtk::Label::new(Some(filename)));
                                let close = gtk::Button::with_label("");
                                let notebook_clone = nt.clone();
                                close.connect_clicked(move |_| {
                                    let page = notebook_clone.page_num(&scrolled);
                                    if page != Some(u32::MAX) {
                                        notebook_clone.remove_page(page);
                                    }
                                });
                                tab.append(&close);
                                close.add_css_class("close-button");
                            }
                            install_autosave(&buffer, path3.to_str().unwrap().to_string());
                            nt.append_page(&scrolled2, Some(&tab));
                            nt.set_tab_reorderable(&scrolled2, true);
                            nt.set_tab_detachable(&scrolled2, true);
                            // build_body(&window_for_dialog, terminal, nt);
                        }
                    }
                    Err(err) => {
                        eprintln!("Save dialog canceled or failed: {err}");
                    }
                }
            },
        );
    });

    window.add_action(&open_action);
    window.add_action(&save_as);
    window.add_action(&newfile);

    let menu_button = MenuButton::builder()
        .label("")
        .menu_model(&menu)
        .has_frame(false)
        .build();
    menu_button.set_always_show_arrow(false);
    menu_button.set_can_shrink(true);
    menu_button.set_has_frame(false);
    let term = gtk::Button::builder()
        .label(" 󱂩 ")
        .build();
    let right = gtk::Button::builder()
        .label("  ")
        .build();
    let _window2 = window.clone();
    let _window3 = window.clone();
    let notebook5 = notebook4.clone();
    let leftright = Paned::new(Orientation::Horizontal);
    let updown = Paned::new(Orientation::Vertical);
    updown.set_hexpand(true);
    updown.set_vexpand(true);
    updown.set_resize_start_child(true);
    updown.set_resize_end_child(true);
    updown.set_shrink_start_child(true);
    updown.set_shrink_end_child(true);
    leftright.set_hexpand(true);
    leftright.set_vexpand(true);
    leftright.set_resize_start_child(true);
    leftright.set_resize_end_child(true);
    leftright.set_shrink_start_child(true);
    leftright.set_shrink_end_child(true);
    leftright.set_start_child(Some(&updown));
    let parent2 = parent.clone();
    world.set_hexpand(true);
    world.set_vexpand(true);
    parent.set_hexpand(true);
    parent.set_vexpand(true);
    toggle_term(updown.clone(), notebook5.clone(), parent2.clone());
    updown.set_start_child(Some(&notebook5));
    parent.append(&leftright);
    let notebook6 = notebook5.clone();
    let parent3 = parent2.clone();
    term.connect_clicked(move |_| {
        TERMINAL.with(|v| v.set(!v.get()));
        toggle_term(updown.clone(), notebook5.clone(), parent2.clone());
    });
    right.connect_clicked(move |_| {
        TERMINAL2.with(|v| v.set(!v.get()));
        toggle_term2(leftright.clone(), notebook6.clone(), parent3.clone());
    });
    let manager = sv::StyleSchemeManager::default();
    manager.append_search_path("themes");
    manager.force_rescan();
    if let Some(scheme) = manager.scheme("catppuccin-mocha") {
        BUFFERS.with(|buffers| {
            for buffer in buffers.borrow().iter() {
                buffer.set_style_scheme(Some(&scheme));
            }
        });
        SCHEME.with(|cell| {
            *cell.borrow_mut() = Some(scheme.clone());
        });
        println!("changed scheme");
    }
    let button = gtk::Button::with_label("");

    let window2 = window.clone();
    button.connect_clicked(move |_| {
        let chooser = sv::StyleSchemeChooserWidget::new();
        let scrolled = ScrolledWindow::new();
        scrolled.set_child(Some(&chooser));
        let dialog = gtk::Dialog::builder()
            .transient_for(&window2)
            .modal(true)
            .default_width(400)
            .default_height(400)
            .title("Choose theme")
            .build();
        scrolled.set_vexpand(true);
        scrolled.set_hexpand(true);
        dialog.content_area().append(&scrolled);
        dialog.add_button("Close", gtk::ResponseType::Close);
        chooser.connect_style_scheme_notify(move |c| {
            let scheme = c.style_scheme();
            BUFFERS.with(|buffers| {
                for buffer in buffers.borrow().iter() {
                    buffer.set_style_scheme(Some(&scheme));
                }
            });
            SCHEME.with(|cell| {
                *cell.borrow_mut() = Some(scheme.clone());
            });
            println!("{}", scheme);
            if let Some(style) = scheme.style("text") {
                if let Some(bg) = style.background() {
                    if let Some(color) = style.foreground() {
                        if let Some(style2) = scheme.style("selection") {
                            if let Some(fg) = style2.background() {
                                update_css(bg.as_str(), color.as_str(), fg.as_str());
                                // println!("There should be css next");
                            }
                        }
                    }
                }
            }
        });

        dialog.connect_response(|d, _| d.close());
        dialog.present();
    });
    menu_button.add_css_class("file");
    // menu_button2.add_css_class("view-btn");
    button.add_css_class("theme");
    term.add_css_class("run");
    right.add_css_class("run");
    let spacer = GtkBox::new(Orientation::Vertical, 0);
    spacer.set_vexpand(true);
    header.append(&menu_button);
    // header.append(&menu_button2);
    header.append(&spacer);
    header.append(&button);
    header.append(&term);
    header.append(&right);
    header.add_css_class("header");
    notebook4.add_css_class("notebook");
    header.set_hexpand(false);
    world.append(&header);
    world.append(&parent);
    window.set_child(Some(&world));
}

fn build_formal(window: &ApplicationWindow, notebook: Notebook) {
    let notebook2 = notebook.clone();
    build_window(&window, notebook2);
}
