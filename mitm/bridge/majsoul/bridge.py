import threading
from typing import Self
from enum import Enum
from functools import cmp_to_key
from .liqi import LiqiProto, MsgType
from ..bridge_base import BridgeBase
from ..logger import logger

# Thread-safe storage for the latest operation list from server.
# Used by autoplay to find the correct chi combination index.
_last_op_lock = threading.Lock()
_last_op_list = []

# Monotonically increasing counter for ActionDiscardTile events,
# so autoplay can detect if a discard was silently ignored.
_discard_counter = 0


def get_last_operation_list():
    """Return the last operation list received from the server."""
    with _last_op_lock:
        return list(_last_op_list)


def get_discard_counter():
    """Return monotonic counter incremented on each ActionDiscardTile."""
    with _last_op_lock:
        return _discard_counter

MS_TILE_2_MJAI_TILE = {
    '0m': '5mr',
    '1m': '1m',
    '2m': '2m',
    '3m': '3m',
    '4m': '4m',
    '5m': '5m',
    '6m': '6m',
    '7m': '7m',
    '8m': '8m',
    '9m': '9m',
    '0p': '5pr',
    '1p': '1p',
    '2p': '2p',
    '3p': '3p',
    '4p': '4p',
    '5p': '5p',
    '6p': '6p',
    '7p': '7p',
    '8p': '8p',
    '9p': '9p',
    '0s': '5sr',
    '1s': '1s',
    '2s': '2s',
    '3s': '3s',
    '4s': '4s',
    '5s': '5s',
    '6s': '6s',
    '7s': '7s',
    '8s': '8s',
    '9s': '9s',
    '1z': 'E',
    '2z': 'S',
    '3z': 'W',
    '4z': 'N',
    '5z': 'P',
    '6z': 'F',
    '7z': 'C'
}
MJAI_TILE_2_MS_TILE = {
    '5mr': '0m',
    '1m': '1m',
    '2m': '2m',
    '3m': '3m',
    '4m': '4m',
    '5m': '5m',
    '6m': '6m',
    '7m': '7m',
    '8m': '8m',
    '9m': '9m',
    '5pr': '0p',
    '1p': '1p',
    '2p': '2p',
    '3p': '3p',
    '4p': '4p',
    '5p': '5p',
    '6p': '6p',
    '7p': '7p',
    '8p': '8p',
    '9p': '9p',
    '5sr': '0s',
    '1s': '1s',
    '2s': '2s',
    '3s': '3s',
    '4s': '4s',
    '5s': '5s',
    '6s': '6s',
    '7s': '7s',
    '8s': '8s',
    '9s': '9s',
    'E': '1z',
    'S': '2z',
    'W': '3z',
    'N': '4z',
    'P': '5z',
    'F': '6z',
    'C': '7z'
}


class Operation:
    NoEffect = 0
    Discard = 1
    Chi = 2
    Peng = 3
    AnGang = 4
    MingGang = 5
    JiaGang = 6
    Liqi = 7
    Zimo = 8
    Hu = 9
    LiuJu = 10

class OperationChiPengGang:
    Chi = 0
    Peng = 1
    Gang = 2

class OperationAnGangAddGang:
    AnGang = 3
    AddGang = 2

class MajsoulBridge(BridgeBase):
    def __init__(self):
        super().__init__()
        self.liqi_proto = LiqiProto()

        self.accountId = 0
        self.seat = 0
        self.lastDiscard = None
        self.reach = False
        self.accept_reach = None
        self.operation = {}
        self.AllReady = False
        self.temp = {}
        self.doras = []
        self.my_tehais = ["?"]*13
        self.my_tsumohai = "?"
        self.syncing = False

        self.mode_id = -1
        self.rank = -1
        self.score = -1

        self.is_3p = False

    def reset(self):
        super().__init__()

        self.accountId = 0
        self.seat = 0
        self.lastDiscard = None
        self.reach = False
        self.accept_reach = None
        self.operation = {}
        self.AllReady = False
        self.temp = {}
        self.doras = []
        self.my_tehais = ["?"]*13
        self.my_tsumohai = "?"
        self.syncing = False

        self.mode_id = -1
        self.rank = -1
        self.score = -1

        self.is_3p = False


    def parse(self, content: bytes) -> None | list[dict]:
        """Parses the content and returns MJAI command.

        Args:
            content (bytes): Content to be parsed.

        Returns:
            None | list[dict]: MJAI command.
        """
        liqi_message = self.liqi_proto.parse(content)
        logger.debug(f"{liqi_message}")
        ret = self.parse_liqi(liqi_message)
        logger.debug(f"-> {ret}")
        return ret

    def parse_liqi(self, liqi_message: dict) -> None | list[dict]:
        ret = []

        if liqi_message is None:
            return None
        # Sync Game
        if ((liqi_message['method'] == '.lq.FastTest.syncGame' or liqi_message['method'] == '.lq.FastTest.enterGame')
            and liqi_message['type'] == MsgType.Res):
            self.syncing = True
            syncGame_msgs = LiqiProto().parse_syncGame(liqi_message)
            parsed_list = []
            parsed = []
            for msg in syncGame_msgs:
                parsed = self.parse_liqi(msg)
                if parsed:
                    parsed_list.extend(parsed)
            self.syncing = False
            if len(parsed_list)>=1:
                return parsed_list
            else:
                ret = []
                return ret
            
        # ready
        if liqi_message['method'] == '.lq.FastTest.fetchGamePlayerState' and liqi_message['type'] == MsgType.Res:
            # if liqi_message['data']['stateList'] == ['READY', 'READY', 'READY', 'READY']:
            self.AllReady = True
            return ret
        # start_game
        if liqi_message['method'] == '.lq.FastTest.authGame' and liqi_message['type'] == MsgType.Req:
            self.reset()
            self.accountId = liqi_message['data']['accountId']
            return ret
        if liqi_message['method'] == '.lq.FastTest.authGame' and liqi_message['type'] == MsgType.Res:
            self.is_3p = len(liqi_message['data']['seatList']) == 3
            try:
                self.mode_id = liqi_message['data']['gameConfig']['meta']['modeId']
            except:
                self.mode_id = -1

            seatList = liqi_message['data']['seatList']
            self.seat = seatList.index(self.accountId)
            ret.append({
                'type': 'start_game',
                'id': self.seat
            })
            return ret
        if liqi_message['method'] == '.lq.ActionPrototype':
            # start_kyoku
            if liqi_message['data']['name'] == 'ActionNewRound':
                self.AllReady = False
                bakaze = ['E', 'S', 'W', 'N'][liqi_message['data']['data']['chang']]
                dora_marker = MS_TILE_2_MJAI_TILE[liqi_message['data']['data']['doras'][0]]
                self.doras = [dora_marker]
                honba = liqi_message['data']['data']['ben']
                oya = liqi_message['data']['data']['ju']
                kyoku = oya + 1
                kyotaku = liqi_message['data']['data']['liqibang']
                scores = liqi_message['data']['data']['scores']
                if self.is_3p:
                    scores = scores + [0]
                tehais = [['?']*13]*4
                my_tehais = ['?']*13
                for hai in range(13):
                    my_tehais[hai] = MS_TILE_2_MJAI_TILE[liqi_message['data']['data']['tiles'][hai]]
                if   len(liqi_message['data']['data']['tiles']) == 13:
                    tehais[self.seat] = sorted(my_tehais, key=cmp_to_key(compare_pai))
                    ret.append(
                        {
                            'type': 'start_kyoku',
                            'bakaze': bakaze,
                            'dora_marker': dora_marker,
                            'honba': honba,
                            'kyoku': kyoku,
                            'kyotaku': kyotaku,
                            'oya': oya,
                            'scores': scores,
                            'tehais': tehais
                        }
                    )
                elif len(liqi_message['data']['data']['tiles']) == 14:
                    self.my_tsumohai = MS_TILE_2_MJAI_TILE[liqi_message['data']['data']['tiles'][13]]
                    all_tehais = my_tehais + [self.my_tsumohai]
                    all_tehais = sorted(all_tehais, key=cmp_to_key(compare_pai))
                    tehais[self.seat] = all_tehais[:13]
                    ret.append(
                        {
                            'type': 'start_kyoku',
                            'bakaze': bakaze,
                            'dora_marker': dora_marker,
                            'honba': honba,
                            'kyoku': kyoku,
                            'kyotaku': kyotaku,
                            'oya': oya,
                            'scores': scores,
                            'tehais': tehais
                        }
                    )
                    ret.append(
                        {
                            'type': 'tsumo',
                            'actor': self.seat,
                            'pai': all_tehais[13]
                        }
                    )
                else:
                    raise

            if self.accept_reach is not None:
                ret.append(self.accept_reach)
                self.accept_reach = None

            # According to mjai.app, in the case of an ankan, the dora event comes first, followed by the tsumo event.
            if 'data' in liqi_message['data']:
                if 'doras' in liqi_message['data']['data']:
                    if len(liqi_message['data']['data']['doras']) > len(self.doras):
                        ret.append(
                            {
                                'type': 'dora',
                                'dora_marker': MS_TILE_2_MJAI_TILE[liqi_message['data']['data']['doras'][-1]]
                            }
                        )
                        self.doras = liqi_message['data']['data']['doras']
                
            # tsumo
            if liqi_message['data']['name'] == 'ActionDealTile':
                actor = liqi_message['data']['data']['seat']
                if liqi_message['data']['data']['tile'] == '':
                    pai = '?'
                else:
                    pai = MS_TILE_2_MJAI_TILE[liqi_message['data']['data']['tile']]
                    self.my_tsumohai = pai
                ret.append(
                    {
                        'type': 'tsumo',
                        'actor': actor,
                        'pai': pai
                    }
                )
            # dahai
            if liqi_message['data']['name'] == 'ActionDiscardTile':
                global _discard_counter
                with _last_op_lock:
                    _discard_counter += 1
                actor = liqi_message['data']['data']['seat']
                self.lastDiscard = actor
                pai = MS_TILE_2_MJAI_TILE[liqi_message['data']['data']['tile']]
                tsumogiri = liqi_message['data']['data']['moqie']
                if liqi_message['data']['data']['isLiqi']:
                    ret.append(
                        {
                            'type': 'reach',
                            'actor': actor
                        }
                    )
                ret.append(
                    {
                        'type': 'dahai',
                        'actor': actor,
                        'pai': pai,
                        'tsumogiri': tsumogiri
                    }
                )
                if liqi_message['data']['data']['isLiqi']:
                    self.accept_reach = {
                                            'type': 'reach_accepted',
                                            'actor': actor
                                        }
            # Reach
            if liqi_message['data']['name'] == 'ActionReach':
                # TODO
                pass
            # ChiPonKan
            if liqi_message['data']['name'] == 'ActionChiPengGang':
                actor = liqi_message['data']['data']['seat']
                target = actor
                consumed = []
                pai = ''
                for idx, seat in enumerate(liqi_message['data']['data']['froms']):
                    if seat != actor:
                        target = seat
                        pai = MS_TILE_2_MJAI_TILE[liqi_message['data']['data']['tiles'][idx]]
                    else:
                        consumed.append(MS_TILE_2_MJAI_TILE[liqi_message['data']['data']['tiles'][idx]])
                assert target != actor
                assert len(consumed) != 0
                assert pai != ''
                match liqi_message['data']['data']['type']:
                    case OperationChiPengGang.Chi:
                        assert len(consumed) == 2
                        ret.append(
                            {
                                'type': 'chi',
                                'actor': actor,
                                'target': target,
                                'pai': pai,
                                'consumed': consumed
                            }
                        )
                        pass
                    case OperationChiPengGang.Peng:
                        assert len(consumed) == 2
                        ret.append(
                            {
                                'type': 'pon',
                                'actor': actor,
                                'target': target,
                                'pai': pai,
                                'consumed': consumed
                            }
                        )
                    case OperationChiPengGang.Gang:
                        assert len(consumed) == 3
                        ret.append(
                            {
                                'type': 'daiminkan',
                                'actor': actor,
                                'target': target,
                                'pai': pai,
                                'consumed': consumed
                            }
                        )
                        pass
                    case _:
                        raise
            # AnkanKakan
            if liqi_message['data']['name'] == 'ActionAnGangAddGang':
                actor = liqi_message['data']['data']['seat']
                match liqi_message['data']['data']['type']:
                    case OperationAnGangAddGang.AnGang:
                        pai = MS_TILE_2_MJAI_TILE[liqi_message['data']['data']['tiles']]
                        consumed = [pai.replace("r", "")]*4
                        if pai[0] == '5' and pai[1] != 'z':
                            consumed[0] += 'r'
                        ret.append(
                            {
                                'type': 'ankan',
                                'actor': actor,
                                'consumed': consumed
                            }
                        )
                    case OperationAnGangAddGang.AddGang:
                        pai = MS_TILE_2_MJAI_TILE[liqi_message['data']['data']['tiles']]
                        consumed = [pai.replace("r", "")] * 3
                        if pai[0] == "5" and not pai.endswith("r"):
                            consumed[0] = consumed[0] + "r"
                        ret.append(
                            {
                                'type': 'kakan',
                                'actor': actor,
                                'pai': pai,
                                'consumed': consumed
                            }
                        )

            if liqi_message['data']['name'] == 'ActionBaBei':
                actor = liqi_message['data']['data']['seat']
                ret.append(
                    {
                        'type': 'nukidora',
                        'actor': actor,
                        'pai': 'N'
                    }
                )

            # hora
            if liqi_message['data']['name'] == 'ActionHule':
                # actor = liqi_message['data']['hules']['seat']
                # if liqi_message['data']['hules']['zimo']:
                #     target = actor
                # else:
                #     target = self.lastDiscard
                # pai = MS_TILE_2_MJAI_TILE[liqi_message['data']['hules']['huTile']]
                # ret.append(
                #     {
                #         'type': 'hora',
                #         'actor': actor,
                #         'pai': pai
                #     }
                # )
                ret = []
                ret.append(
                    {
                        'type': 'end_kyoku'
                    }
                )
                return ret
            # notile
            if liqi_message['data']['name'] == 'ActionNoTile':
                ret = []
                ret.append(
                    {
                        'type': 'end_kyoku'
                    }
                )
                return ret
            # ryukyoku
            if liqi_message['data']['name'] == 'ActionLiuJu':
                # ret.append(
                #     {
                #         'type': 'ryukyoku'
                #     }
                # )
                ret = []
                ret.append(
                    {
                        'type': 'end_kyoku'
                    }
                )
                return ret
            if 'data' in liqi_message['data']:
                if 'operation' in liqi_message['data']['data']:
                    global _last_op_list
                    op_data = liqi_message['data']['data']['operation']
                    with _last_op_lock:
                        _last_op_list = op_data.get('operationList', [])
                    return ret
        # end_game
        if liqi_message['method'] == '.lq.NotifyGameEndResult' or liqi_message['method'] == '.lq.NotifyGameTerminate':
            try:
                for idx, player in enumerate(liqi_message['data']['result']['players']):
                    if player['seat'] == self.seat:
                        self.rank = idx + 1
                        self.score = player['partPoint1']
            except:
                pass
            ret.append(
                {
                    'type': 'end_game'
                }
            )
            return ret
        return ret
        
    
    def build(self, command: dict) -> None | bytes:
        pass

def compare_pai(pai1: str, pai2: str):
    # Smallest
    # 1m~4m, 5mr, 5m~9m,
    # 1p~4p, 5pr, 5p~9p,
    # 1s~4s, 5sr, 5s~9s,
    # E, S, W, N, P, F, C, ?
    # Biggest
    pai_order = [
        '1m', '2m', '3m', '4m', '5mr', '5m', '6m', '7m', '8m', '9m',
        '1p', '2p', '3p', '4p', '5pr', '5p', '6p', '7p', '8p', '9p',
        '1s', '2s', '3s', '4s', '5sr', '5s', '6s', '7s', '8s', '9s',
        'E', 'S', 'W', 'N', 'P', 'F', 'C', '?'
    ]
    idx1 = pai_order.index(pai1)
    idx2 = pai_order.index(pai2)
    if idx1 > idx2:
        return 1  
    elif idx1 == idx2:
        return 0
    else:
        return -1