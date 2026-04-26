# Nerd IDE
NerdIDE is an IDE written in Rust which uses GTK-4 and VTE. It's goal is to provide the experience one can get in JetBrains IDEs but as software that takes up less space and is not so strict about system requirments, as it is quite easy to find a laptop / pc which will be lagging when a heavy IDE is open.

## Dependencies
NerdIDE uses the JetBrainsMono Nerd Font for displaying icons and text in the editor, so you will need to install it. Follow the instructions on https://nerdfonts.com
Of course, you will need rust. Install it in any way you like, i.e. as described on https://rustup.rs.
All other things - GTK4, Sourceview5 and stuff should be compiled by Cargo, so we don't need to worry about that.

## Installation
Clone this repo with
`git clone https://github.com/frozen-infinity/NerdIDE.git`
Then cd into the cloned dir with 
`cd NerdIDE/`
Now a simple `cargo run` should do the trick. If you encounter problems with GTK, try changing the version specified in Cargo.toml, it doesn't affect anything.


## TODOs
+ Add errors highlighting
+ Add bash / sh brackets and indent autocompletion
+ Capture the world!!!
