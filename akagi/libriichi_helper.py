import numpy as np

def meta_to_recommend(meta: dict, is_3p=False) -> dict:
    # """
    # {
    #     "q_values":[
    #         -9.09196,
    #         -9.46696,
    #         -8.365397,
    #         -8.849772,
    #         -9.43571,
    #         -10.06071,
    #         -9.295085,
    #         -0.73649096,
    #         -9.27946,
    #         -9.357585,
    #         0.3221028,
    #         -2.7794597
    #     ],
    #     "mask_bits":2697207348,
    #     "is_greedy":true,
    #     "eval_time_ns":357088300
    # }
    # """

    recommend = []

    mask_unicode_4p = [
        "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m",
        "1p", "2p", "3p", "4p", "5p", "6p", "7p", "8p", "9p",
        "1s", "2s", "3s", "4s", "5s", "6s", "7s", "8s", "9s",
         "E",  "S",  "W",  "N",  "P",  "F",  "C",
        '5mr', '5pr', '5sr', 
        'reach', 'chi_low', 'chi_mid', 'chi_high', 'pon', 'kan_select', 'hora', 'ryukyoku', 'none'
    ]
    mask_unicode_3p = [
        "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m",
        "1p", "2p", "3p", "4p", "5p", "6p", "7p", "8p", "9p",
        "1s", "2s", "3s", "4s", "5s", "6s", "7s", "8s", "9s",
         "E",  "S",  "W",  "N",  "P",  "F",  "C",
        '5mr', '5pr', '5sr', 
        'reach', 'pon', 'kan_select', 'nukidora', 'hora', 'ryukyoku', 'none'
    ]
    if is_3p:
        mask_unicode = mask_unicode_3p
    else:
        mask_unicode = mask_unicode_4p
    
    def mask_bits_to_binary_string(mask_bits):
        binary_string = bin(mask_bits)[2:]
        binary_string = binary_string.zfill(46)
        return binary_string

    def mask_bits_to_bool_list(mask_bits):
        binary_string = mask_bits_to_binary_string(mask_bits)
        bool_list = []
        for bit in binary_string[::-1]:
            bool_list.append(bit == '1')
        return bool_list

    def eq(l, r):
        # Check for approximate equality using numpy's floating-point epsilon
        return np.abs(l - r) <= np.finfo(float).eps

    def softmax(arr, temperature=1.0):
        arr = np.array(arr, dtype=float)  # Ensure the input is a numpy array of floats
        
        if arr.size == 0:
            return arr  # Return the empty array if input is empty

        if not eq(temperature, 1.0):
            arr /= temperature  # Scale by temperature if temperature is not approximately 1

        # Shift values by max for numerical stability
        max_val = np.max(arr)
        arr = arr - max_val
        
        # Apply the softmax transformation
        exp_arr = np.exp(arr)
        sum_exp = np.sum(exp_arr)
        
        softmax_arr = exp_arr / sum_exp
        
        return softmax_arr

    def scale_list(list):
        scaled_list = softmax(list)
        return scaled_list
    q_values = meta['q_values']
    mask_bits = meta['mask_bits']
    mask = mask_bits_to_bool_list(mask_bits)
    scaled_q_values = scale_list(q_values)
    q_value_idx = 0

    true_count = 0
    for i in range(46):
        if mask[i]:
            true_count += 1

    for i in range(46):
        if mask[i]:
            recommend.append((mask_unicode[i], scaled_q_values[q_value_idx]))
            q_value_idx += 1

    recommend = sorted(recommend, key=lambda x: x[1], reverse=True)
    return recommend

def state_to_tehai(state) -> tuple[list[str], str]:
    tehai34 = state.tehai # with tsumohai, no aka marked
    akas = state.akas_in_hand
    tsumohai = state.last_self_tsumo()
    return _state_to_tehai(tehai34, akas, tsumohai)

def _state_to_tehai(tile34: int, aka: list[bool], tsumohai: str|None) -> tuple[list[str], str]:
    pai_str = [
        "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m",
        "1p", "2p", "3p", "4p", "5p", "6p", "7p", "8p", "9p",
        "1s", "2s", "3s", "4s", "5s", "6s", "7s", "8s", "9s",
         "E",  "S",  "W",  "N",  "P",  "F",  "C",  "?"
    ]
    aka_str = [
        "5mr", "5pr", "5sr"
    ]
    tile_list = []
    for tile_id, tile_count in enumerate(tile34):
        for _ in range(tile_count):
            tile_list.append(pai_str[tile_id])
    for idx, aka in enumerate(aka):
        if aka:
            tile_list[tile_list.index("5" + ["m", "p", "s"][idx])] = aka_str[idx]
    if len(tile_list)%3 == 2 and tsumohai is not None:
        tile_list.remove(tsumohai)
    else:
        tsumohai = "?"
    len_tile_list = len(tile_list)
    if len_tile_list < 13:
        tile_list += ["?"]*(13-len_tile_list)

    return (tile_list, tsumohai)
    