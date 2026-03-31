#!/bin/sh

echo "Should make output only if there's a duplicate panic handler, which is only if std is present:"
cargo check 2>&1 | grep "panic_impl"
