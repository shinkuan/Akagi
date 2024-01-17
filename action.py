import cv2
import numpy as np
import pyautogui
import pygetwindow as gw
import time
import random
from PIL import Image, ImageGrab
from convert import MS_TILE_2_MJAI_TILE
from functools import cmp_to_key
from majsoul2mjai import compare_pai
from loguru import logger


class Action:
    def __init__(self):
        self.isNewRound = True
        self.reached = False
        self.latest_operation_list = []
        pass


    def decide_random_time(self):
        if self.isNewRound:
            return random.uniform(2.3, 2.5)
        return random.uniform(0.3, 1.2)

    def get_window(self):
        window_titles = ['Majsoul Plus', '雀魂', 'Majsoul']
        for window_title in window_titles:
            window = gw.getWindowsWithTitle(window_title)
            if len(window) > 0:
                window = window[0]
                if window and window.isMinimized == False:
                    return window
        return None
            

    def get_ltrb(self, window):
        w = window.width/16
        h = window.height/9
        if w > h:
            # Height is the limiting factor
            height = window.height
            width = height*16/9
            l = window.left + (window.width-width)/2
            t = window.top
            r = window.right - (window.width-width)/2
            b = window.bottom
        else:   
            # Width is the limiting factor
            width = window.width
            height = width*9/16
            l = window.left
            t = window.top + (window.height-height)/2
            r = window.right
            b = window.bottom - (window.height-height)/2
        l = l if l > 0 else 0
        t = t if t > 0 else 0
        r = r if r < pyautogui.size().width else pyautogui.size().width
        b = b if b < pyautogui.size().height else pyautogui.size().height
        return (int(l), int(t), int(r), int(b))


    def move_back_to_lt(self, window):
        ltrb = self.get_ltrb(window)
        pyautogui.moveTo(ltrb[0]+100, ltrb[1]+100, tween=pyautogui.easeInOutQuad, duration=0.4)
        return


    def click_object_on_screen(self, window, object_image_path: str):
        time.sleep(0.5)
        ltrb = self.get_ltrb(window)
        print(ltrb)
        screen = ImageGrab.grab(bbox=ltrb)
        # Convert to numpy array
        screen = np.array(screen)
        object_scale = (ltrb[3]-ltrb[1])/900 # 900 is the height of the object
        object_img = cv2.imread(object_image_path+".png", cv2.IMREAD_COLOR)
        object_img = cv2.cvtColor(object_img, cv2.COLOR_BGR2RGB)
        object_img = cv2.resize(object_img, None, fx=object_scale, fy=object_scale, interpolation=cv2.INTER_AREA)

        scale_factors = np.linspace(0.8, 1.15, 8)

        best_match = None
        best_scale = 1
        max_val = -1

        for scale in scale_factors:
            # 調整物件圖像的大小
            resized_object = cv2.resize(object_img, None, fx=scale, fy=scale, interpolation=cv2.INTER_AREA)

            # 使用模板匹配在每個大小級別上進行匹配
            res = cv2.matchTemplate(screen, resized_object, cv2.TM_CCOEFF_NORMED, mask=resized_object.copy())
            min_val, max_val_scale, min_loc, max_loc_scale = cv2.minMaxLoc(res)

            # 更新最佳匹配和相應的縮放因子
            if max_val_scale > max_val:
                max_val = max_val_scale
                best_match = max_loc_scale
                best_scale = scale

        max_loc = best_match
        max_val = max_val
        click_loc = (int(max_loc[0] + resized_object.shape[1] / 2 + ltrb[0]), int(max_loc[1] + resized_object.shape[0] / 2 + ltrb[1]))
        pyautogui.moveTo(click_loc, tween=pyautogui.easeInOutQuad, duration=0.2)
        time.sleep(0.1)
        pyautogui.click()
        time.sleep(0.1)
        self.move_back_to_lt(window)
        return


    def click_chiponkan(self, mjai_msg: dict | None, tehai: list[str], tsumohai: str | None):
        action = mjai_msg['type']
        if action == 'none':
            object_path = './objects/none'
        elif action == 'chi':
            object_path = './objects/chi'
        elif action == 'pon':
            object_path = './objects/pon'
        elif action == 'daikakan':
            object_path = './objects/daikakan'
        elif action == 'ankan':
            object_path = './objects/ankan'
        elif action == 'kakan':
            object_path = './objects/kakan'
        elif action == 'hora':
            logger.log("CLICK", tsumohai)
            if tsumohai.startswith("?"):
                object_path = './objects/hora'
                logger.log("CLICK", "ron")
            else:
                object_path = './objects/tsumo'
                logger.log("CLICK", "tsumo")
        elif action == 'reach':
            object_path = './objects/reach'
        elif action == 'ryukyoku':
            object_path = './objects/ryukyoku'
        else:
            return
        self.click_object_on_screen(self.get_window(), object_path)
        if action == 'reach':
            self.reached = True
            time.sleep(0.5)
            self.click_dahai(mjai_msg, tehai, tsumohai)


    def click_chipon_candidates(self, mjai_msg: dict | None, tehai: list[str], tsumohai: str | None):
        consumed_pais_mjai = mjai_msg['consumed']
        consumed_pais_mjai = sorted(consumed_pais_mjai, key=cmp_to_key(compare_pai))
        if mjai_msg['type'] == 'chi':
            for operation in self.latest_operation_list:
                if operation['type'] == 2:
                    combination_len = len(operation['combination'])
                    if combination_len == 1:
                        return  # No need to click
                    for idx, combination in enumerate(operation['combination']):
                        consumed_pais_liqi = [MS_TILE_2_MJAI_TILE[pai] for pai in combination.split('|')]
                        consumed_pais_liqi = sorted(consumed_pais_liqi, key=cmp_to_key(compare_pai))
                        if consumed_pais_mjai == consumed_pais_liqi:
                            time.sleep(0.3)
                            self.click_candidate(combination_len, idx)
                            pass
        elif mjai_msg['type'] == 'pon':
            for operation in self.latest_operation_list:
                if operation['type'] == 3:
                    combination_len = len(operation['combination'])
                    if combination_len == 1:
                        return
                    for idx, combination in enumerate(operation['combination']):
                        consumed_pais_liqi = [MS_TILE_2_MJAI_TILE[pai] for pai in combination.split('|')]
                        consumed_pais_liqi = sorted(consumed_pais_liqi, key=cmp_to_key(compare_pai))
                        if consumed_pais_mjai == consumed_pais_liqi:
                            time.sleep(0.3)
                            self.click_candidate(combination_len, idx)


    def click_candidate(self, candidate_len: int, idx: int):
        window = self.get_window()
        ltrb = self.get_ltrb(window)
        w = ltrb[2] - ltrb[0]
        h = ltrb[3] - ltrb[1]
        if candidate_len <= 6:
            candidate_y_cord = h * 0.696 + ltrb[1]
            candidate_width = w * 0.104
            middle_x_cord = ltrb[0] + w * 0.5
            candidate_x_cord = middle_x_cord + candidate_width * (idx - (candidate_len/2 - 0.5))
            pyautogui.moveTo(candidate_x_cord, candidate_y_cord, tween=pyautogui.easeInOutQuad, duration=0.4)
            time.sleep(0.1)
            pyautogui.click()
            time.sleep(0.1)
            self.move_back_to_lt(window)
        pass


    def get_pai_coord(self, idx: int, tehais: list[str]):
        tehai_count = 0
        for tehai in tehais:
            if tehai != '?':
                tehai_count += 1
        window = self.get_window()
        ltrb = self.get_ltrb(window)
        w = ltrb[2] - ltrb[0]
        h = ltrb[3] - ltrb[1]
        pai_y_cord = h * 0.9 + ltrb[1]

        pai_width = w * 0.049479
        pai_start_x_cord = ltrb[0] + w * 0.138802

        if idx == 13:
            pai_x_cord = pai_start_x_cord + pai_width * (tehai_count) + w * 0.015625
        else:
            pai_x_cord = pai_start_x_cord + pai_width * (idx)
        
        return (int(pai_x_cord), int(pai_y_cord))


    def click_dahai(self, mjai_msg: dict | None, tehai: list[str], tsumohai: str | None):
        dahai = mjai_msg['pai']
        if self.isNewRound:
            temp_tehai = tehai.copy()
            temp_tehai.append(tsumohai)
            temp_tehai = sorted(temp_tehai, key=cmp_to_key(compare_pai))
            for i in range(14):
                if dahai == temp_tehai[i]:
                    pai_coord = self.get_pai_coord(i, temp_tehai)
                    pyautogui.moveTo(pai_coord, tween=pyautogui.easeInOutQuad, duration=0.4)
                    time.sleep(0.1)
                    pyautogui.click()
                    time.sleep(0.1)
                    self.move_back_to_lt(self.get_window())
                    self.isNewRound = False
                    return
        if tsumohai is not None:
            if dahai == tsumohai:
                pai_coord = self.get_pai_coord(13, tehai)
                pyautogui.moveTo(pai_coord, tween=pyautogui.easeInOutQuad, duration=0.4)
                time.sleep(0.1)
                pyautogui.click()
                time.sleep(0.1)
                self.move_back_to_lt(self.get_window())
                return
        for i in range(13):
            if dahai == tehai[i]:
                pai_coord = self.get_pai_coord(i, tehai)
                pyautogui.moveTo(pai_coord, tween=pyautogui.easeInOutQuad, duration=0.4)
                time.sleep(0.1)
                pyautogui.click()
                time.sleep(0.1)
                self.move_back_to_lt(self.get_window())
                break
        

    def mjai2action(self, mjai_msg: dict | None, tehai: list[str], tsumohai: str | None):
        dahai_delay = self.decide_random_time()
        if mjai_msg is None:
            return
        if mjai_msg['type'] in ['none', 'chi', 'pon', 'daiminkan', 'ankan', 'kakan', 'hora', 'reach', 'ryukyoku']:
            self.click_chiponkan(mjai_msg, tehai, tsumohai)
            if mjai_msg['type'] in ['chi', 'pon']:
                self.click_chipon_candidates(mjai_msg, tehai, tsumohai)
            return
        if mjai_msg['type'] == 'dahai' and not self.reached:
            time.sleep(dahai_delay)
            self.click_dahai(mjai_msg, tehai, tsumohai)
            return