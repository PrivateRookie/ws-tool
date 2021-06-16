docker run -it --rm \
    -v "${PWD}/test_config:/config" \
    -v "${PWD}/test_reports:/reports" \
    -p 9001:9001 \
    --name fuzzingserver \
    crossbario/autobahn-testsuite \
    wstest --mode "fuzzingserver" -d --spec "/config/fuzzingserver.json"
