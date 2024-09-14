set -xe

cargo build --target=wasm32-unknown-unknown --release
wasm-bindgen target/wasm32-unknown-unknown/release/proglad_visualizer.wasm --out-dir www --target web
