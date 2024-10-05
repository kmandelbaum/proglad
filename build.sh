set -xe
cargo build --release

cd visualizer-svg
bash build.sh
cd ..
