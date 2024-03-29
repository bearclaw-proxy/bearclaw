#!/usr/bin/env python3

import capnp
import os
import sys
import time

capnp.remove_import_hook()
bearclaw_capnp = capnp.load('../../bearclaw.capnp')

newMessages = False
nextIndex = 0

def unwrap(obj):
    which = obj.which()
    if which == 'ok':
        return obj.ok
    else:
        print('Error code returned:')
        print(obj.err)
        sys.exit(1)

def getMessages(historySearch):
    global nextIndex
    while True:
        items = historySearch.getItems(nextIndex, 10).wait().items

        for id in items:
            item = unwrap(bearclaw.getHistoryItem(id).wait().result)
            nextIndex += 1
            printHistoryItem(item)

        if len(items) < 10:
            break
    
def printHistoryItem(item):
    print('')
    print(item.connectionInfo().wait())
    print(item.requestTimestamp().wait())
    print(item.requestBytes().wait())
    print(item.responseTimestamp().wait())
    printResponse(item.responseBytes().wait().responseBytes)

def printResponse(response):
    which = response.which()

    if which == 'ok':
        print("First 1,000 bytes of response:")
        print(response.ok[0:1000])
    else:
        print(response.err)

class HistorySubscriberImpl(bearclaw_capnp.HistorySubscriber.Server):
    def notifyNewItem(self, **kwargs):
        global newMessages
        print('Subscription notification received')
        newMessages = True

print('Connecting to bearclaw-proxy RPC...')

client = capnp.TwoPartyClient(os.environ.get('BEARCLAW_RPC_ENDPOINT', 'localhost:3092'))
bearclaw = client.bootstrap().cast_as(bearclaw_capnp.Bearclaw)

print('')
print('Creating proxy history search...')

historySearch = bearclaw.searchHistory().wait().historySearch;

print('')
print('Subscribing to proxy history search notifications...')

subscriber = HistorySubscriberImpl()
subscription = historySearch.subscribe(subscriber).wait().subscription;

print('')
print('Downloading proxy history...')
print(historySearch.getCount().wait())

getMessages(historySearch)

print('')
print('Waiting for new proxy history items...')

while True:
    capnp.poll_once()
    time.sleep(0.25)
    if newMessages:
        newMessages = False
        getMessages(historySearch)
