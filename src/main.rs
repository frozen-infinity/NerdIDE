use gtk::Separator;
use gtk::gio;
use gtk::prelude::*;
use gtk::{
    Application, ApplicationWindow, Box as GtkBox, Button, CssProvider, FlowBox, HeaderBar,
    MenuButton, Orientation, STYLE_PROVIDER_PRIORITY_APPLICATION, ScrolledWindow, gdk, glib,
};
use sourceview5 as sv;
use sourceview5::prelude::*;
use std::fs;
use std::process::Command;
use std::sync::{LazyLock, Mutex};
const APP_ID: &str = "nerd.ide.gtk4rs";
/*fn load_css() {
    let provider = CssProvider::new();
    provider.load_from_path("src/style.css");

    let display = gdk::Display::default().expect("Could not connect to a display");
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
} */
use sourceview5::Buffer;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

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
    /*app.connect_startup(|_| {
        load_css();
    });*/
    app.connect_activate(|app| {
        build_ui(app, false);
    });
    app.run()
}

fn build_ui(app: &Application, build_footer: bool) {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("IDE")
        .default_width(500)
        .default_height(400)
        .build();
    let window_clone = window.clone();
    build_body(&window_clone, false, "/Users/natano/main.py");
    window.present();
}

fn build_header(window: &ApplicationWindow, buffer: Buffer) -> GtkBox {
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
        println!("This is new file");
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
                            println!("Save path: {}", path.display());
                            let start = buffer2.start_iter();
                            let end = buffer2.end_iter();
                            let text = buffer2.text(&start, &end, false);
                            fs::File::create(&path);
                            println!("New file: {}", path.display());
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
                        println!("Save path: {}", path.display());
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
    // end of openas
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
                        println!("Chosen file path: {}", path.display());
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
    let menu_button = MenuButton::builder().label("IDE").menu_model(&menu).build();
    header.append(&menu_button);
    header
}

fn build_body(window: &ApplicationWindow, file_tree: bool, file_path: &str) {
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
    let view = sv::View::with_buffer(&buffer);
    view.set_show_line_numbers(true);
    view.set_highlight_current_line(true);
    view.set_auto_indent(true);
    view.set_insert_spaces_instead_of_tabs(false);
    view.set_tab_width(4);
    view.set_monospace(true);
    view.set_enable_snippets(true);

    install_br(&view, &buffer);

    let view_for_focus = view.clone();
    glib::idle_add_local_once(move || {
        view_for_focus.grab_focus();
    });
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
    let header = build_header(&window, buffer);
    parent.append(&header);
    parent.set_vexpand(true);
    parent.append(&body);
    window.set_child(Some(&parent));
}
