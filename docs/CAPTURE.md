# Sending traffic to sip-source

sip-source receives SIP, RTCP and logs over HEP (versions 2 and 3), on UDP and TCP port 9060. Any
server or agent that speaks HEP can send to it: Asterisk, OpenSIPS, Kamailio, FreeSWITCH or a
packet capture agent such as heplify.

## Asterisk

In `/etc/asterisk/hep.conf` on each Asterisk server:

```ini
[general]
enabled=yes
capture_address=SIP_SOURCE_IP:9060
capture_id=2001
uuid_type=call-id
```

Give every server its own `capture_id`. Keep `uuid_type=call-id`, because RTCP reports are matched
to calls by it. Then load the modules:

```bash
asterisk -rx "module load res_hep.so"
```

```bash
asterisk -rx "module load res_hep_pjsip.so"
```

```bash
asterisk -rx "module load res_hep_rtcp.so"
```

`res_hep_pjsip` captures PJSIP only, not `chan_sip`.

If you set `SS_HEP_AUTH_KEY`, set the same value as `capture_password` in `hep.conf`. Packets
without the key are dropped.

## OpenSIPS, Kamailio, FreeSWITCH and capture agents

Point the HEP output of the server, or of a capture agent running next to it, at port 9060 of
sip-source, with a capture ID per server. HEP version 3 is preferred; version 2 also works.

## Sources

Every address that sends HEP appears under **Admin → Sources** with its first packet. Give each one
an alias there, such as `Asterisk-1` or `OpenSIPS-edge`. The source menu in the top bar then limits
dashboards, search and live capture to one server. In search, `source:10.0.0.5` does the same.

## Joining call legs

A call through a back-to-back user agent such as Asterisk has two SIP dialogs. To join them
exactly, pass the incoming leg's Call-ID to the outgoing INVITE in a header. On Asterisk:

```ini
[from-phones]
exten => _X.,1,Set(__ACALLID=${CHANNEL(pjsip,call-id)})
 same => n,Dial(PJSIP/${EXTEN}@carrier,20,b(add-cid^s^1))

[add-cid]
exten => s,1,Set(PJSIP_HEADER(add,X-CID)=${ACALLID})
 same => n,Return()
```

The header name is set by **Correlation headers** under **Admin → Settings → Capture** (`X-CID` by
default). Without such a header, legs are matched by capture agent, timing and called number, and
those calls are marked `inferred`.

## Direction and trunks

Under **Admin → Settings → Topology**, list your carriers and subscriber networks as `Name=CIDR`
pairs, for example `CarrierX=203.0.113.0/24`. Calls then get a direction (inbound, outbound, on-net)
and a trunk. Until this is set, direction shows as `unknown` and trunk charts stay empty.
