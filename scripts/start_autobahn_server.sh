docker run -it --rm \
    -v "${PWD}/test_config:/config" \
    -v "${PWD}/test_reports:/reports" \
    -p 9002:9002 \
    --name fuzzingserver \
    'crossbario/autobahn-testsuite:0.8.0' \
    wstest --mode "fuzzingserver" -d --spec "/config/fuzzingserver.json"
