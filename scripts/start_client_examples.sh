set -e

# docker run -it --rm \
#     -v "${PWD}/test_config:/config" \
#     -v "${PWD}/test_reports:/reports" \
#     -p 9002:9002 \
#     --name fuzzingserver \
#     'crossbario/autobahn-testsuite' \
#     wstest --mode "fuzzingserver" --spec "/config/fuzzingserver.json"

for ex in autobahn_client autobahn_async_client autobahn_deflate_client autobahn_async_deflate_client; do
cargo run --example $ex --all-features --release;
done

docker stop fuzzingserver
