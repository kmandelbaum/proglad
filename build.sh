set -xe
cd server
cargo build --release
cd ..

cd visualizer-svg
bash build.sh
cd ..
