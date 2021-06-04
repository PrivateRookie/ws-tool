#!/bin/bash


openssl genrsa -des3 -out myCA.key 2048


openssl req -x509 -new -nodes -key myCA.key -sha256 -days 1825 -out myCA.pem

#openssl req -newkey rsa:2048 -new -nodes -x509 -days 3650 -keyout key.pem -out cert.pem
