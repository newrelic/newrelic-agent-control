#! /bin/sh

rm ./dist/newrelic-supervisor_linux_arm64/newrelic-supervisor

cargo build

cp ./target/debug/main ./dist/newrelic-supervisor_linux_arm64/newrelic-supervisor