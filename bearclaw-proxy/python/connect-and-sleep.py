#!/usr/bin/env python3

import capnp
import sys
import time

capnp.remove_import_hook()
bearclaw_capnp = capnp.load('../../bearclaw.capnp')

print('Connecting to bearclaw-proxy RPC...')

client = capnp.TwoPartyClient('localhost:3092')
bearclaw = client.bootstrap().cast_as(bearclaw_capnp.Bearclaw)

while True:
    capnp.poll_once()
    time.sleep(0.25)
