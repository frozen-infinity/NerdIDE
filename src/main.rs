use gtk::Separator;
use gtk::gio;
use gtk::prelude::*;
use gtk::{
    Application, ApplicationWindow, Box as GtkBox, Button, CssProvider, FlowBox, HeaderBar,
    MenuButton, Orientation, STYLE_PROVIDER_PRIORITY_APPLICATION, ScrolledWindow, gdk, glib,
};
use sourceview5 as sv;
use sourceview5::prelude::*;
use sourceview5::StyleSchemeChooserButton;
use std::fs;
use vte4 as vte;
use vte::prelude::*;
use std::process::Command;
use std::sync::{LazyLock, Mutex};
use gtk::{Settings};

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
static tab_width: &str = "    ";
const APP_ID: &str = "nerd.ide.gtk4rs";

use sourceview5::{Buffer, View};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;
use gtk::ffi::GtkButton;
use gtk::prelude::*;

thread_local! {
    static VARS_PROVIDER: RefCell<Option<CssProvider>> = const { RefCell::new(None) };
}

fn load_css() {
    let static_provider = CssProvider::new();
    static_provider.load_from_path("src/style.css");

    let vars_provider = CssProvider::new();
    vars_provider.load_from_string(":root { --bg: #1e1e2e; }");

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

fn update_css(color: &str, text: &str) {
    let contents = format!(":root {{ --bg: {color}AA; --text: {text}; --btnbg: {color}; }}", color=color, text=text);
    println!("{}", contents);
    VARS_PROVIDER.with(|slot| {
        if let Some(provider) = slot.borrow().as_ref() {
            provider.load_from_string(&contents);
        }
    });
}
fn install_autosave(buffer: &sv::Buffer, path: String) {
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

        let id = glib::timeout_add_local_once(Duration::from_millis(700), move || {
            let (start, end) = buffer_for_save.bounds();
            let text = buffer_for_save.text(&start, &end, true);

            match std::fs::write(path_for_save.as_str(), text.as_str()) {
                Ok(()) => {
                    buffer_for_save.set_modified(false);
                    println!("autosaved");
                }
                Err(err) => {
                    eprintln!("autosave failed: {err}");
                }
            }

            *pending_save_for_save.borrow_mut() = None;
        });

        *pending_save_clone.borrow_mut() = Some(id);
    });
}

fn install_br(view: &sv::View, buffer: &sv::Buffer) {
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
            _ => return glib::Propagation::Proceed,
        };
        let closing = match ch {
            "(" => Some(")"),
            "[" => Some("]"),
            "{" => Some("}"),
            "\"" => Some("\""),
            "'" => Some("'"),
            _ => None,
        };
        if ch != ")" && ch != "]" && ch != "}" {
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
        //if !end.forward_char() {
          //  return glib::Propagation::Proceed;
        //}
        let next = buffer.text(&start, &end, false);
        println!("next {}", next);
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
        glib::Propagation::Proceed
    });
    println!("I am adding this");
    view.add_controller(key);
}

fn main() -> glib::ExitCode {
    let app = Application::builder().application_id(APP_ID).build();
    app.connect_startup(|app| {
        load_css();
    });
    app.connect_activate(|app| {
        build_ui(app, false);
    });
    app.run()
}

fn language_formatting(lang: &str, view: &sv::View, buffer: &sv::Buffer) {
    let key = gtk::EventControllerKey::new();
    key.set_propagation_phase(gtk::PropagationPhase::Capture);

    let buffer = buffer.clone();
    println!("{}", lang);
    let lang = lang.clone();
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

        let unit = tab_width;
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
fn build_ui(app: &Application, build_footer: bool) {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("IDE")
        .default_width(500)
        .default_height(400)
        .build();
    let window_clone = window.clone();
    build_body(&window_clone, false, "/Users/natano/CLionProjects/prog/main.cpp");
    window.present();
    set_caret_style();
}

fn build_header(window: &ApplicationWindow, buffer: Buffer, view: &View, path: &str, terminal: bool) -> GtkBox {
    let header = GtkBox::new(Orientation::Horizontal, 10);

    let menu = gio::Menu::new();
    menu.append(Some("Open"), Some("win.open"));
    menu.append(Some("Save as"), Some("win.saveas"));
    menu.append(Some("New File"), Some("win.newfile"));

    let window_clone = window.clone();
    let save_as = gio::SimpleAction::new("saveas", None);
    let newfile = gio::SimpleAction::new("newfile", None);

    let bf = buffer.clone();
    newfile.connect_activate(move |_, _| {
        let dialog = gtk::FileDialog::builder()
            .title("New File")
            .modal(true)
            .accept_label("Select")
            .build();

        let buffer2 = bf.clone();
        let window2 = window_clone.clone();

        dialog.save(
            Some(&window_clone),
            None::<&gio::Cancellable>,
            move |result| {
                let window3 = window2.clone();
                match result {
                    Ok(file) => {
                        if let Some(path) = file.path() {
                            let _ = fs::File::create(&path);
                            build_body(&window3, true, path.to_str().unwrap());
                        }
                    }
                    Err(err) => {
                        eprintln!("Save dialog canceled or failed: {err}");
                    }
                }
            },
        );
    });

    let buffer1 = buffer.clone();
    let wc = window.clone();
    save_as.connect_activate(move |_, _| {
        let dialog = gtk::FileDialog::builder()
            .title("Save As")
            .modal(true)
            .accept_label("Save")
            .build();

        let buffer2 = buffer1.clone();
        let window2 = wc.clone();

        dialog.save(
            Some(&window2),
            None::<&gio::Cancellable>,
            move |result| match result {
                Ok(file) => {
                    if let Some(path) = file.path() {
                        let start = buffer2.start_iter();
                        let end = buffer2.end_iter();
                        let text = buffer2.text(&start, &end, false);
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

    let window_clone2 = window.clone();
    let open_action = gio::SimpleAction::new("open", None);
    open_action.connect_activate(move |_, _| {
        let dialog = gtk::FileDialog::builder()
            .title("Choose a file")
            .modal(true)
            .build();

        let window_for_dialog = window_clone2.clone();

        dialog.open(
            Some(&window_clone2),
            None::<&gio::Cancellable>,
            move |result| match result {
                Ok(file) => {
                    if let Some(path) = file.path() {
                        let path_string = path.to_string_lossy().to_string();
                        build_body(&window_for_dialog, false, &path_string);
                    } else {
                        println!("Chosen file has no local path");
                        println!("URI: {}", file.uri());
                    }
                }
                Err(err) => {
                    eprintln!("File dialog canceled or failed: {err}");
                }
            },
        );
    });

    window.add_action(&open_action);
    window.add_action(&save_as);
    window.add_action(&newfile);

    let menu_button = MenuButton::builder()
        .label("File")
        .menu_model(&menu)
        .has_frame(false)
        .build();

    let view_menu = gio::Menu::new();
    view_menu.append(Some("Toggle line numbers"), Some("win.linenumbers"));

    let ln = gio::SimpleAction::new("linenumbers", None);
    let view = view.clone();
    ln.connect_activate(move |_, _| {
        view.set_show_line_numbers(!view.shows_line_numbers());
    });
    window.add_action(&ln);

    let menu_button2 = MenuButton::builder()
        .label("View")
        .menu_model(&view_menu)
        .has_frame(false)
        .build();

    let run = gtk::Button::builder()
        .label("Term")
        .build();
    let window2 = window.clone();
    let path2 = path.to_string();
    run.connect_clicked(move |_| {
        build_body(&window2, !terminal, path2.as_str());
    });

    let manager = sv::StyleSchemeManager::default();
    manager.append_search_path("themes");
    manager.force_rescan();

    let theme_btn = sv::StyleSchemeChooserButton::new();

    if let Some(scheme) = manager.scheme("catppuccin-mocha") {
        buffer.set_style_scheme(Some(&scheme));
        theme_btn.set_style_scheme(&scheme);
    }

    let buffer_for_theme = buffer.clone();
    theme_btn.connect_style_scheme_notify(move |btn| {
        let scheme = btn.style_scheme();
        buffer_for_theme.set_style_scheme(Some(&scheme));
        if let Some(style) = scheme.style("text") {
            if let Some(bg) = style.background() {
                if let Some(color) = style.foreground() {
                    update_css(bg.as_str(), color.as_str());
                }
            }
        }    });
    menu_button.add_css_class("file");
    menu_button2.add_css_class("view-btn");
    theme_btn.add_css_class("theme");
    run.add_css_class("run");
    let spacer = GtkBox::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    header.append(&menu_button);
    header.append(&menu_button2);
    header.append(&spacer);
    header.append(&theme_btn);
    header.append(&run);
    header.add_css_class("header");
    header
}

fn build_body(window: &ApplicationWindow, terminal: bool, file_path: &str) {
    let body = GtkBox::new(Orientation::Horizontal, 6);
    let file = gio::File::for_path(file_path);
    let source_file = sv::File::new();
    source_file.set_location(Some(&file));
    let lm = sv::LanguageManager::default();
    let mut buffer = sv::Buffer::new(None);
    if let Some(lang) = lm.guess_language(Some(file_path), None) {
        buffer = sv::Buffer::with_language(&lang);
    }
    let gio_file = gio::File::for_path(file_path);
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
    let spaces = tab_width.chars().filter(|&c| c == ' ').count() as u32;
    let indent_width = spaces as i32;
    let view = sv::View::with_buffer(&buffer);
    view.set_show_line_numbers(true);
    view.set_highlight_current_line(true);
    view.set_auto_indent(true);
    view.set_indent_width(indent_width);
    view.set_tab_width(spaces);
    view.set_insert_spaces_instead_of_tabs(true);
    view.set_indent_on_tab(true);
    view.set_smart_backspace(true);
    view.set_monospace(true);
    if let Some(lang) = lm.guess_language(Some(file_path), None) {
        language_formatting(lang.to_string().as_str(), &view, &buffer);
    }
    install_br(&view, &buffer);

    let scrolled = ScrolledWindow::builder()
        .child(&view)
        .min_content_height(300)
        .build();
    scrolled.set_vexpand(true);
    scrolled.set_hexpand(true);
    install_autosave(&buffer, file_path.to_string());
    body.append(&scrolled);
    body.set_vexpand(true);
    body.set_hexpand(true);
    let parent = GtkBox::new(Orientation::Vertical, 6);
    let header = build_header(&window, buffer, &view, &file_path, terminal);
    parent.append(&header);
    parent.set_vexpand(true);
    parent.append(&body);
    if terminal {
        let term = vte::Terminal::new();
        term.set_hexpand(true);
        term.set_vexpand(true);
        term.set_scrollback_lines(10_000);
        term.set_scroll_on_output(true);
        term.set_input_enabled(true);

        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
        let argv = [shell.as_str(), "-i"];

        term.spawn_async(
            vte::PtyFlags::DEFAULT,
            None,
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

        parent.append(&term);
    } else {
        let view_for_focus = view.clone();
        glib::idle_add_local_once(move || {
            view_for_focus.grab_focus();
        });
    }
    parent.add_css_class("parent");
    window.add_css_class("window");
    view.add_css_class("view");
    window.set_child(Some(&parent));
}
