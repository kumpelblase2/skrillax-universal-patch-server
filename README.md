# Skrillax Universal Patch Server

This is a minimal Gateway Server implementation for Silkroad Online
which allows patching both ways; update and downgrade. I.e. using
this, the client can go from v1.594 to v1.595 OR v1.593, depending
on what the desired version is. To properly use this, you most likely
need a special patcher tool that can connect to the patch server and
handle patch selection for it.

> [!WARNING] Work in progress
> This is very much a work in progress. There's no error correction,
> error messages, or other helpful stuff provided by the server. It
> will probably break, so treat it with care.

## Requirements

What you need additionally (in short):

- Full patch content for each version
- Simple file server

For the Universal Patch Server to work, it needs all files with their
content for _all_ the versions available through the server, in addition
to the _full_ file list for the base version. The latter part isn't
strictly necessary, but could cause files to remain unpatched when
downgrading.

The server needs a folder called `patches`, which should contain a
directory for each available patch version[^1]. Inside each patch folder
should be the individual files making up the patch using the same paths
and filenames as provided by the official patch server. The files should
also already be compressed, as this patch server does not serve the files
themselves.

You additionally need a normal (static) file server that can serve the
actual files to patch to the client. Any server is fine - nginx,
miniserve, whatever works.

## Usage

Create a directory and place both the `patches` directory and the patcher
executable inside. From within that directory, run the patcher. The patcher
will open sockets on ports between 32000 and 32999[^2], so make sure these are
accessible from the outside and unused.

Start the file server and configure it to serve the content _inside_ the
`patches` directory from port 80 (this generally elevated privileges).

The rest should be up to the client patcher.

## How it works

Silkroad Online normally does not support downgrading by itself, as it's
not really something that they want to do, as it's simpler to just update
and revert the changes. However, the client doesn't actually care if the
new version is larger or smaller than the current version. It can be tricked
into "updating" to an older version, by simply telling it there's an update
and sending it an older version as the new one.

We then only need a way for the client to indicate which version it wants
to change to. This is done through the port. When a client connects to the
port `32594` we take it as a request to patch to version `594`. In the
patching process, the client then sends its current version, which we can
compare to what version it wants. We can then diff the files between the
two versions and send the new files accordingly.

The only problem being the client will _always_ use port 15779 for the
gateway. To work around that, we need a custom patcher that will temporarily
run a proxy that will pick the right port instead, such that the client
still connects to 15779 and is then forwarded to the corresponding port
for the version.

[^1]: If the client reports `v1.594`, the patch version is `594`. This is the value the client sends as its version.
[^2]: For each version available, the patcher will open a port given the pattern `32XXX` where `XXX` is the version. For
version 594 the port would be `32594`.