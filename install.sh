#!/bin/bash

echo "Building in release mode..."

cargo build --release

if [ $? -ne 0 ]; then
    echo "Build failed! :("
    exit 1
else
    echo "Done!"
    echo "Copying binary to ~/.cargo/bin..."

    cp target/release/brownhttpd ~/.cargo/bin

    echo "Done!"

    echo "Generating zsh completions to ~/.zfunc..."

    brownhttpd --gen-completions zsh > ~/.zsh-completions/_brownhttpd

    echo "Done!"
    exit 0
fi
