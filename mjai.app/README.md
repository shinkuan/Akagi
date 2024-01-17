# mjai-simulator

[AI Jansou](https://mjai.app) is a platform for mahjong AI competition.
This repository contains the implementation of mahjong game simulator for evaluating submission files on AI Jansou.

## Usage

You can simulate a mahjong game by specifying submission files as shown in the code below.
The Simulator runs [Docker](https://www.docker.com/) internally. The `docker` command must be installed and be available to run with user privileges.

```py
import mjai

submissions = [
    "examples/shanten.zip",
    "examples/tsumogiri.zip",
    "examples/tsumogiri.zip",
    "examples/invalidbot2.zip",
]
mjai.Simulator(submissions, logs_dir="./logs").run()
```

### Docker Image

Submission files are deployed in a Docker container and run as a Docker container.

This repository contains Dockerfile and other resources that will be used to create the Docker container.
The docker image is pushed to Docker Hub (`docker.io/smly/mjai-client:v3`).

### Submission format

Please prepare a program that outputs the appropriate [mjai protocol message](https://gimite.net/pukiwiki/index.php?Mjai%20%E9%BA%BB%E9%9B%80AI%E5%AF%BE%E6%88%A6%E3%82%B5%E3%83%BC%E3%83%90) to the standard output when given input in the mjai protocol format from standard input. Name this program "bot.py" and pack it into a zip file. The zip file should contain bot.py directly under the root directory.

bot.py must be a Python script, but it is also possible to include precompiled libraries if they are executable. The program will be executed in a `linux/amd64` environment. The submission file must be 1000 MB or less.
bot.py takes a player ID as its first argument. Player ID must be 0, 1, 2, or 3. Player ID 0 represents the chicha 起家, and subsequent numbers represent the shimocha 下家, toimen 対面 or kamicha 上家 of the chicha 起家. See [example code](https://github.com/smly/mjai.app/blob/main/examples/tsumogiri/bot.py) for details.

### Timeout

When the `mjai.Simulator` instance creates an environment to run the submission file in docker, it specifies the `--platform linux/amd64` option. If you want to run on a different architecture, you will have to emulate and run the container, which will be much slower. If you are debugging on an architecture other than `linux/amd64`, you can avoid timeout errors by relaxing the timeout limit. Specify the `timeout` argument as follows. The `timeout` is set to 2.0 by default.

```py
Simulator(submissions, logs_dir="./logs", timeout=10.0).run()
```

If the simulation is interrupted, the docker container may not terminate successfully. If past docker containers remain, the new docker container may fail to launch due to duplicate HTTP ports. You can remove all docker containers that have the `smly/mjai-client` image as an ancestor in a batch as follows:

```sh
% for cid in `docker ps -a --filter ancestor=smly/mjai-client:v3 --format "{{.ID}}"`; do docker rm -f $cid; done
```

## Protocol

The protocol is basically based on the [Mjai Protocol](https://gimite.net/pukiwiki/index.php?Mjai%20%E9%BA%BB%E9%9B%80AI%E5%AF%BE%E6%88%A6%E3%82%B5%E3%83%BC%E3%83%90), but customized based on [Mortal's Mjai Engine implementation](https://github.com/Equim-chan/Mortal/blob/main/libriichi/src/mjai/event.rs). The following points are customized:

- Messages are sent and received by standard input and standard output.
- The game server sends messages collectively up to the event that the player can act on.
- When the player does not send the appropriate event message, it is treated as a chombo and a mangan-sized penalty is paid.
- If the player does not send a message within 2 seconds, it is treated as a chombo and a mangan-sized penalty is paid.
- The first kyoku is not necessarily East 1.

### The first kyoku is not necessarily East 1

エラーが発生した局は流局扱いとなり、すべてのプレイヤーのプログラムは終了させられる。
すべてのプレイヤーのプログラムは再起動され、エラーが発生した次の局からゲームが再開される。
そのためプレイヤーのプログラムが初めて受け取る局は東 1 局とは限らない。

The following are messages sent and received as seen from player 0.
`<-` denotes events for the player.
`->` denotes events from the player.

```
# Game resumed from S1-1
0 <- [{"type":"start_game","names":["0","1","2","3"],"id":0}]
0 -> {"type":"none"}
# S1-1 流局による終局イベント (first actionable event が来る前に局が終わる)
# NOTE: No ryukyoku events are sent from the game server.
0 <- [{"type":"end_kyoku"}]
0 -> {"type":"none"}
# S2-2 first tsumo tile
0 <- [{"type":"start_kyoku","bakaze":"S","dora_marker":"1p","kyoku":2,"honba":2,"kyotaku":0,"oya":1,"scores":[800,61100,11300,26800],"tehais":[["4p","4s","P","3p","1p","5s","2m","F","1m","7s","9m","6m","9s"],["?","?","?","?","?","?","?","?","?","?","?","?","?"],["?","?","?","?","?","?","?","?","?","?","?","?","?"],["?","?","?","?","?","?","?","?","?","?","?","?","?"]]},{"type":"tsumo","actor":1,"pai":"?"},{"type":"dahai","actor":1,"pai":"F","tsumogiri":false},{"type":"tsumo","actor":2,"pai":"?"},{"type":"dahai","actor":2,"pai":"3m","tsumogiri":true},{"type":"tsumo","actor":3,"pai":"?"},{"type":"dahai","actor":3,"pai":"1m","tsumogiri":true},{"type":"tsumo","actor":0,"pai":"3s"}]
0 -> {"type":"dahai","pai":"3s","actor":0,"tsumogiri":true}
```

### Case Study: Furiten (振聴)

When the player has already made a tenpai but the hand is furiten. Since the player cannot Ron, even if the waiting tile is discarded by the opponent, no action is possible.
For example, let's say you have `2333678m 678s 678p`. The waiting tile is `14m` and `1m` has already been discarded.
Since the hand is a Furiten, even if the other player discards `1m`, the player cannot Ron.

```
3 <- [{"type":"dahai","actor":3,"pai":"P","tsumogiri":true},{"type":"tsumo","actor":0,"pai":"?"},{"type":"dahai","actor":0,"pai":"2p","tsumogiri":true},{"type":"tsumo","actor":1,"pai":"?"},{"type":"dahai","actor":1,"pai":"4m","tsumogiri":true},{"type":"tsumo","actor":2,"pai":"?"},{"type":"dahai","actor":2,"pai":"6m","tsumogiri":true}]
```

In this case, immediately after actor 2 discards 6m, input is given to actor 3. actor 3 needs to decide whether to call `chi` on 6m.

### Case Study: Ankan (暗槓)

In the case of an ankan, the dora event comes first, followed by the tsumo event.

```
3 -> {"type": "ankan", "actor": 3, "consumed": ["6s", "6s", "6s", "6s"]}
3 <- [{"type":"ankan","actor":3,"consumed":["6s","6s","6s","6s"]},{"type":"dora","dora_marker":"6p"},{"type":"tsumo","actor":3,"pai":"7p"}]
3 -> {"type":dahai","actor":3,"pai":"7p","tsumogiri":true}
```

## For Developers

### Debug with interactive shell

The procedures executed by Simulator can be checked and debugged one by one as follows:

```bash
# pull latest docker image
% docker pull docker.io/smly/mjai-client:v3

# launch
% CONTAINER_ID=`docker run -d --rm -p 28080:3000 --mount "type=bind,src=/Users/smly/gitws/mjai.app/examples/rulebase.zip,dst=/bot.zip,readonly" smly/mjai-client:v3 sleep infinity`

# install
% docker exec ${CONTAINER_ID} unzip -q /bot.zip
% docker cp python/mjai/http_server/server.py ${CONTAINER_ID}:/workspace/00__server__.py

# debug
% docker exec -it ${CONTAINER_ID} /workspace/.pyenv/shims/python -u bot.py 0
[{"type":"start_game","id":0}]  <-- Input
{"type":"none"}  <-- Output
[{"type":"start_kyoku","bakaze":"E","dora_marker":"2s","kyoku":1,"honba":0,"kyotaku":0,"oya":0,"scores":[25000,25000,25000,25000],"tehais":[["E","6p","9m","8m","C","2s","7m","S","6m","1m","S","3s","8m"],["?","?","?","?","?","?","?","?","?","?","?","?","?"],["?","?","?","?","?","?","?","?","?","?","?","?","?"],["?","?","?","?","?","?","?","?","?","?","?","?","?"]]},{"type":"tsumo","actor":0,"pai":"1m"}]  <-- Input
{"type": "dahai", "actor": 0, "pai": "C", "tsumogiri": false}  <-- Output
[{"type":"dahai","actor":0,"pai":"C","tsumogiri":false},{"type":"tsumo","actor":1,"pai":"?"},{"type":"dahai","actor":1,"pai":"3m","tsumogiri":false},{"type":"tsumo","actor":2,"pai":"?"},{"type":"dahai","actor":2,"pai":"1m","tsumogiri":false}]  <-- Input
{"type": "none"}  <-- Output
[{"type":"tsumo","actor":3,"pai":"?"},{"type":"dahai","actor":3,"pai":"1m","tsumogiri":false}]  <-- Input
{"type": "none"}  <-- Output
[{"type":"tsumo","actor":0,"pai":"C"}]  <-- Input
{"type": "dahai", "actor": 0, "pai": "C", "tsumogiri": true}  <-- Output
```

### Debug with http sever

```bash
# http server mode. `0` is the player index.
% docker exec -it ${CONTAINER_ID} /workspace/.pyenv/shims/python 00__server__.py 0
```

```bash
% curl http://localhost:28080/
OK

% curl -X POST -d '[{"type":"start_game","id":0}]' http://localhost:28080/
{"type":"none"}

% cat > request.json
[{"type":"start_kyoku","bakaze":"E","dora_marker":"2s","kyoku":1,"honba":0,"kyotaku":0,"oya":0,"scores":[25000,25000,25000,25000],"tehais":[["E","6p","9m","8m","C","2s","7m","S","6m","1m","S","3s","8m"],["?","?","?","?","?","?","?","?","?","?","?","?","?"],["?","?","?","?","?","?","?","?","?","?","?","?","?"],["?","?","?","?","?","?","?","?","?","?","?","?","?"]]},{"type":"tsumo","actor":0,"pai":"1m"}]

% curl -X POST -d '@request.json' http://localhost:28080/
{"type":"dahai","actor":0,"pai":"6p","tsumogiri":false}
```

## Development

Confirmed working with rustc 1.70.0 (90c541806 2023-05-31).

## Special Thanks

The code in `./src` directory is Mortal's libriichi with minor updates. Mortal is distributed under the AGPL-3.0 and is copyrighted by Equim.
