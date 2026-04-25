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
use gtk::ffi::gtk_header_bar_remove;

thread_local! {
    static VARS_PROVIDER: RefCell<Option<CssProvider>> = const { RefCell::new(None) };
}

fn load_css() {
    let static_provider = CssProvider::new();
    static_provider.load_from_path("src/style.css");

    let vars_provider = CssProvider::new();
    vars_provider.load_from_string(":root { --bg: #1e1e2eAA; --text: #cdd6f4; --btnbg: #1e1e2e; }");

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

        let id = glib::timeout_add_local_once(Duration::from_millis(700), move || {
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
        .title("IDE")
        .default_width(500)
        .default_height(400)
        .build();

    let notebook = gtk::Notebook::new();
    notebook.set_scrollable(true);
    notebook.set_hexpand(true);
    notebook.set_vexpand(true);

    let file_path = "/Users/natano/CLionProjects/prog/main.cpp";

    let lm = sv::LanguageManager::default();
    let mut buffer = Buffer::new(None);
    if let Some(lang) = lm.guess_language(Some(file_path), None) {
        buffer = Buffer::with_language(&lang);
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

    let tab = gtk::Label::new(Some("main.cpp"));
    notebook.append_page(&scrolled, Some(&tab));

    build_body(&window, false, notebook);

    window.present();
    set_caret_style();
}

fn build_header(window: &ApplicationWindow, terminal: bool, notebook: Notebook) -> GtkBox {
    let header = GtkBox::new(Orientation::Vertical, 10);
    let window0 = window.clone();
    let notebook0 = notebook.clone();
    let notebook4 = notebook.clone();
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
                let window3 = window2.clone();
                match result {
                    Ok(file) => {
                        if let Some(path) = file.path() {
                            let path2 = path.clone();
                            let path3 = path.clone();
                            let _ = fs::File::create(&path);
                            let mut filename: &str;
                            let mut tab = gtk::Box::new(Orientation::Horizontal, 0);
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

                            let scrolled = ScrolledWindow::builder()
                                .child(&view)
                                .min_content_height(300)
                                .build();
                            scrolled.set_vexpand(true);
                            scrolled.set_hexpand(true);
                            install_autosave(&buffer, path3.to_str().unwrap().to_string());
                            let path21 = path3.to_str().unwrap().to_string();
                            let scrolled2 = scrolled.clone();
                            if let Some(index) = path21.rfind('/') {
                                filename = &path21[index + 1..];
                                tab.append(&gtk::Label::new(Some(filename)));
                                let close = gtk::Button::with_label("x");
                                let notebook_clone = notebook2.clone();
                                close.connect_clicked(move |_| {
                                    let page = notebook_clone.page_num(&scrolled);
                                    if page != Some(u32::MAX) {
                                        notebook_clone.remove_page(page);
                                    }
                                });
                                tab.append(&close);
                            }
                            notebook2.append_page(&scrolled2, Some(&tab));
                            notebook2.set_tab_reorderable(&scrolled2, true);
                            notebook2.set_tab_detachable(&scrolled2, true);
                            build_body(&window3, terminal, notebook2);
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
        let window_for_dialog = window0.clone();
        dialog.open(
            Some(&window0),
            None::<&gio::Cancellable>,
            move |result| {
                match result {
                    Ok(file) => {
                        if let Some(path) = file.path() {
                            let path2 = path.clone();
                            let path3 = path.clone();
                            let mut filename: &str;
                            let mut tab = gtk::Box::new(gtk::Orientation::Horizontal, 8);
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

                            let scrolled = ScrolledWindow::builder()
                                .child(&view)
                                .min_content_height(300)
                                .build();
                            scrolled.set_vexpand(true);
                            scrolled.set_hexpand(true);
                            let path21 = path3.to_str().unwrap().to_string();
                            let scrolled2 = scrolled.clone();
                            if let Some(index) = path21.rfind('/') {
                                filename = &path21[index + 1..];
                                tab.append(&gtk::Label::new(Some(filename)));
                                let close = gtk::Button::with_label("x");
                                let notebook_clone = nt.clone();
                                close.connect_clicked(move |_| {
                                    let page = notebook_clone.page_num(&scrolled);
                                    if page != Some(u32::MAX) {
                                        notebook_clone.remove_page(page);
                                    }
                                });
                                tab.append(&close);
                            }
                            install_autosave(&buffer, path3.to_str().unwrap().to_string());
                            nt.append_page(&scrolled2, Some(&tab));
                            nt.set_tab_reorderable(&scrolled2, true);
                            nt.set_tab_detachable(&scrolled2, true);
                            build_body(&window_for_dialog, terminal, nt);
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
        .label("File")
        .menu_model(&menu)
        .has_frame(false)
        .build();
/*
    let view_menu = gio::Menu::new();
    view_menu.append(Some("Reformat Code"), Some("win.format"));
    view_menu.append(Some("Toggle line numbers"), Some("win.linenumbers"));

    let ln = gio::SimpleAction::new("linenumbers", None);
    let view1 = view.clone();
    ln.connect_activate(move |_, _| {
        view1.set_show_line_numbers(!view1.shows_line_numbers());
    });
    window.add_action(&ln);
    let menu_button2 = MenuButton::builder()
        .label("View")
        .menu_model(&view_menu)
        .has_frame(false)
        .build();
*/
    let term = gtk::Button::builder()
        .label("Term")
        .build();
    let run = gtk::Button::builder()
        .label("Run")
        .build();
    run.add_css_class("run");
    let window2 = window.clone();
    // let path2 = path.to_string();
    let window3 = window.clone();
    // let path3 = path2.to_string();
    // let path4 = path.to_string();
    //let view2 = view.clone();
    let a = terminal;
    term.connect_clicked(move |_| {
        build_body(&window2, !terminal, notebook4.clone());
    });
    let manager = sv::StyleSchemeManager::default();
    manager.append_search_path("themes");
    manager.force_rescan();

    let theme_btn = sv::StyleSchemeChooserButton::new();

    /*if let Some(scheme) = manager.scheme("catppuccin-mocha") {
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
        }
    });
    */
    menu_button.add_css_class("file");
    // menu_button2.add_css_class("view-btn");
    theme_btn.add_css_class("theme");
    term.add_css_class("run");
    run.add_css_class("run");
    let spacer = GtkBox::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    header.append(&menu_button);
    // header.append(&menu_button2);
    header.append(&spacer);
    header.append(&theme_btn);
    header.append(&run);
    header.append(&term);
    header.add_css_class("header");
    header.set_hexpand(false);
    header
}

fn build_body(window: &ApplicationWindow, terminal: bool, notebook: Notebook) {
    let world = GtkBox::new(Orientation::Horizontal, 6);
    let parent = GtkBox::new(Orientation::Horizontal, 6);
    let body = GtkBox::new(Orientation::Horizontal, 6);
    let notebook2 = notebook.clone();
    let header = build_header(&window, terminal, notebook2);
    parent.append(&header);
    body.append(&notebook);
    if terminal {
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
        term.add_css_class("term");
        let paned = Paned::new(Orientation::Vertical);
        paned.set_start_child(Some(&body));
        paned.set_end_child(Some(&term));
        paned.set_hexpand(true);
        paned.set_position(400);
        parent.append(&paned);
    } else {
        parent.append(&body);
    }
    parent.add_css_class("parent");
    window.add_css_class("window");
    // view.add_css_class("view");
    // world.append(&sidebar);
    world.append(&parent);
    window.set_child(Some(&world));
}