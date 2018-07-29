#### Workflow execution

[![Build Status](https://travis-ci.org/gterzian/simple_chat.svg?branch=master)](https://travis-ci.org/gterzian/simple_chat)

##### How to run:

1. `curl -sSf https://static.rust-lang.org/rustup.sh | sh`
2. `git clone git@github.com:gterzian/simple_chat.git`
3. `cd simple_chat`
4. In one terminal tab do: `cargo run --release -- server`
5. In another tab do: `cargo run -- client`
6. Messages and roundtrip info are printed to the console.
