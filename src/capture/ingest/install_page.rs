//! The install page: what "Install userscript" opens.
//!
//! Opening `/akagi.user.js` directly only works when the browser has a
//! userscript manager to intercept it; without one the browser just shows a
//! page of JavaScript and leaves the user guessing. So the button opens this
//! page instead. It links the script for anyone who has a manager, and
//! tells everyone else what is missing and where to get it. That covers the
//! case where the default browser is not the one the manager lives in, too.
//!
//! Plain static HTML, no script. The only input is `?lang=`, and it only
//! picks which built-in strings to use; nothing from the request is
//! echoed into the page.

use super::userscript::USERSCRIPT_PATH;

struct Strings {
    html_lang: &'static str,
    title: &'static str,
    intro: &'static str,
    install: &'static str,
    no_manager_title: &'static str,
    no_manager: &'static str,
    chrome_title: &'static str,
    chrome: &'static str,
    after_title: &'static str,
    after: &'static str,
}

const EN: Strings = Strings {
    html_lang: "en",
    title: "Install the Akagi userscript",
    intro: "Akagi uses a userscript to read the WebSocket traffic of the supported games in this browser. It only reads; nothing is modified.",
    install: "Install userscript",
    no_manager_title: "Only seeing a page of code?",
    no_manager: "Then this browser has no userscript manager yet. Install one of these, come back to this page and click the button again:",
    chrome_title: "Chrome / Edge",
    chrome: "After installing the manager, open chrome://extensions (edge://extensions on Edge), open the manager's Details and turn on “Allow user scripts”. Without it the manager cannot run any script.",
    after_title: "Then",
    after: "Open — or reload — the game page. Akagi shows a “Userscript connected” notification.",
};

const JA: Strings = Strings {
    html_lang: "ja",
    title: "Akagi ユーザースクリプトのインストール",
    intro: "Akagi はユーザースクリプトで、このブラウザ上の対象ゲームの WebSocket 通信を読み取ります。読み取りのみで、変更はしません。",
    install: "ユーザースクリプトをインストール",
    no_manager_title: "コードのページが表示されるだけの場合",
    no_manager: "このブラウザにはまだユーザースクリプトマネージャーがありません。次のどちらかをインストールしてから、このページに戻ってもう一度ボタンを押してください：",
    chrome_title: "Chrome / Edge",
    chrome: "マネージャーをインストールしたら、chrome://extensions（Edge では edge://extensions）を開き、マネージャーの「詳細」で「ユーザー スクリプトを許可する」をオンにしてください。オフのままではスクリプトを実行できません。",
    after_title: "その後",
    after: "ゲームページを開く（または再読み込みする）と、Akagi に「Userscript connected」の通知が表示されます。",
};

const ZH_TW: Strings = Strings {
    html_lang: "zh-Hant",
    title: "安裝 Akagi userscript",
    intro: "Akagi 透過 userscript 讀取這個瀏覽器裡指定遊戲的 WebSocket，只讀取、不修改。",
    install: "安裝 userscript",
    no_manager_title: "按下去只看到一堆程式碼？",
    no_manager: "代表這個瀏覽器還沒有 userscript 管理器。請先安裝下面其中一個，再回到這一頁重新按一次按鈕：",
    chrome_title: "Chrome / Edge",
    chrome: "裝好管理器後，打開 chrome://extensions（Edge 是 edge://extensions），進入管理器的「詳細資料」，開啟「允許使用者指令碼」。沒開的話管理器無法執行任何腳本。",
    after_title: "接著",
    after: "開啟（或重新整理）遊戲頁面，Akagi 會跳出「Userscript connected」通知。",
};

const ZH_CN: Strings = Strings {
    html_lang: "zh-Hans",
    title: "安装 Akagi userscript",
    intro: "Akagi 通过 userscript 读取这个浏览器里指定游戏的 WebSocket，只读取、不修改。",
    install: "安装 userscript",
    no_manager_title: "点击后只看到一堆代码？",
    no_manager: "说明这个浏览器还没有 userscript 管理器。请先安装下面其中一个，再回到这个页面重新点击按钮：",
    chrome_title: "Chrome / Edge",
    chrome: "装好管理器后，打开 chrome://extensions（Edge 是 edge://extensions），进入管理器的「详细信息」，开启「允许用户脚本」。不开启的话管理器无法运行任何脚本。",
    after_title: "然后",
    after: "打开（或刷新）游戏页面，Akagi 会弹出「Userscript connected」通知。",
};

fn strings(lang: Option<&str>) -> &'static Strings {
    match lang.unwrap_or("") {
        "ja" => &JA,
        "zh-TW" => &ZH_TW,
        "zh-CN" => &ZH_CN,
        _ => &EN,
    }
}

/// The install page in `lang` (one of Akagi's UI languages; anything else
/// falls back to English).
pub fn render(lang: Option<&str>) -> String {
    let s = strings(lang);
    format!(
        r#"<!doctype html>
<html lang="{html_lang}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<style>
  :root {{ color-scheme: light dark; }}
  body {{ font-family: system-ui, sans-serif; max-width: 40rem; margin: 3rem auto; padding: 0 1.25rem; line-height: 1.6; }}
  h1 {{ font-size: 1.5rem; }}
  h2 {{ font-size: 1.05rem; margin-top: 2rem; }}
  .install {{ display: inline-block; margin: 1rem 0; padding: .75rem 1.5rem; border-radius: .5rem; background: #2563eb; color: #fff; font-weight: 600; text-decoration: none; }}
  .install:hover {{ background: #1d4ed8; }}
  code {{ font-family: ui-monospace, monospace; }}
</style>
</head>
<body>
<h1>{title}</h1>
<p>{intro}</p>
<p><a class="install" href="{script}">{install}</a></p>
<h2>{no_manager_title}</h2>
<p>{no_manager}</p>
<ul>
  <li><a href="https://www.tampermonkey.net/">Tampermonkey</a></li>
  <li><a href="https://violentmonkey.github.io/">Violentmonkey</a></li>
</ul>
<h2>{chrome_title}</h2>
<p>{chrome}</p>
<h2>{after_title}</h2>
<p>{after}</p>
</body>
</html>
"#,
        html_lang = s.html_lang,
        title = s.title,
        intro = s.intro,
        script = USERSCRIPT_PATH,
        install = s.install,
        no_manager_title = s.no_manager_title,
        no_manager = s.no_manager,
        chrome_title = s.chrome_title,
        chrome = s.chrome,
        after_title = s.after_title,
        after = s.after,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_links_the_script_and_the_managers() {
        for lang in ["en", "ja", "zh-TW", "zh-CN"] {
            let page = render(Some(lang));
            assert!(page.contains(r#"href="/akagi.user.js""#), "{lang}");
            assert!(page.contains("https://www.tampermonkey.net/"), "{lang}");
            assert!(page.contains("https://violentmonkey.github.io/"), "{lang}");
            assert!(page.contains("chrome://extensions"), "{lang}");
        }
    }

    #[test]
    fn languages_pick_their_own_strings() {
        assert!(render(Some("ja")).contains(r#"<html lang="ja">"#));
        assert!(render(Some("zh-TW")).contains("安裝 userscript"));
        assert!(render(Some("zh-CN")).contains("安装 userscript"));
    }

    /// The query only selects built-in strings; nothing from it reaches the
    /// page.
    #[test]
    fn unknown_languages_fall_back_to_english_without_echoing() {
        let page = render(Some("<script>alert(1)</script>"));
        assert!(page.contains(r#"<html lang="en">"#));
        assert!(!page.contains("alert(1)"));
        assert_eq!(render(None), render(Some("en")));
    }
}
