#!/bin/bash

sysctl -w net.ipv4.conf.eth0.route_localnet=1
iptables -t nat -A PREROUTING -i eth0 -p tcp --dport 25 -j DNAT --to-destination 127.0.0.1:9025
