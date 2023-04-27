#!/usr/bin/env python3

import capnp
import os
import sys

capnp.remove_import_hook()
bearclaw_capnp = capnp.load('../../bearclaw.capnp')

print('Connecting to bearclaw-proxy RPC...')

client = capnp.TwoPartyClient(os.environ.get('BEARCLAW_RPC_ENDPOINT', 'localhost:3092'))
bearclaw = client.bootstrap().cast_as(bearclaw_capnp.Bearclaw)

print('Requesting build info...')
print('')

print(bearclaw.getBuildInfo().wait().buildInfo)
