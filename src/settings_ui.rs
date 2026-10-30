//! The settings window's logic that isn't drawing: the search index and its Turkish folding,
//! fullscreen exception names, the timer presets, team crest stand-ins (design:
//! docs/superpowers/designs/2026-10-20-settings-v2/notes.md §5.12, §5.16, §9).

use crate::activities::buds::Paired;
use crate::i18n::{n_, tr, trf};
use crate::settings::BudsDevice;

/// Sidebar order; `SearchHit::page` and the window's `Store.page` index this.
pub const PAGES: [&str; 11] = [n_("Görünüm"), n_("Genel"), n_("Medya"), n_("Zamanlayıcı"), n_("Galaxy Buds"), n_("Canlı maç"), n_("Tam ekran"), n_("Hakkında"), n_("Sistem"), n_("Oyunlar"), n_("Dijital Denge")];

/// One searchable row: page, row key (the window scrolls to it), section, title, description,
/// extra words people might type.
pub struct Entry {
    pub page: u8,
    pub row: &'static str,
    pub section: &'static str,
    pub title: &'static str,
    pub desc: &'static str,
    pub keywords: &'static str,
}

const fn e(page: u8, row: &'static str, section: &'static str, title: &'static str, desc: &'static str, keywords: &'static str) -> Entry {
    Entry { page, row, section, title, desc, keywords }
}

/// Every setting row, in page order (notes §10).
pub const INDEX: &[Entry] = &[
    e(0, "pill-size", n_("Pil"), n_("Pil boyutu"), n_("Yükseklik 32 DIP"), "büyük küçük normal boyut"),
    e(0, "pill-offset", n_("Pil"), n_("Konum"), n_("Görev çubuğunun ortasından sola ya da sağa"), "ortala kaydır sol sağ yer"),
    e(0, "pill-opacity", n_("Pil"), n_("Zemin opaklığı"), n_("Zamanlayıcı, kronometre, Buds ve maç pillerinde; medya pili kapağın renginde kalır"), "saydamlık şeffaflık opacity zemin"),
    e(0, "pill-slivers", n_("Pil"), n_("Arkadaki etkinlikleri göster"), n_("Öndeki pilin sağından taşan ince şeritler"), "şerit yığın arka"),
    e(0, "wallpaper-accent", n_("Renk"), n_("Duvar kâğıdından"), n_("Vurgu rengi duvar kâğıdının renginden gelir; gri bir duvar kâğıdında seçtiğin renk"), "duvar kağıdı arka plan masaüstü renk palet one ui material you"),
    e(0, "accent", n_("Renk"), n_("Vurgu rengi"), n_("Zamanlayıcı, kronometre ve seçici"), "renk accent mor mavi"),
    e(0, "cover-color", n_("Renk"), n_("Kapak rengini kullan"), n_("Medya pili ve kartı albüm kapağının rengini alır; kapalıyken uygulamanın rengini"), "albüm kapak renk"),
    e(0, "card-size", n_("Kart"), n_("Kart boyutu"), n_("Kompakt ya da büyük"), "kompakt büyük boyut"),
    e(0, "card-blur", n_("Kart"), n_("Bulanıklık"), n_("Kartın arkasındaki masaüstü bulanıklaşır"), "blur saydam akrilik malzeme"),
    e(0, "darkness", n_("Kart"), n_("Malzeme koyuluğu"), n_("Bulanık zeminin ne kadar karartılacağı"), "koyu karanlık malzeme"),
    e(0, "saturation", n_("Kart"), n_("Renk doygunluğu"), n_("Bulanık zeminin renk canlılığı"), "doygunluk canlı malzeme saturation"),
    e(0, "speed", n_("Hareket"), n_("Animasyon hızı"), n_("Pilin büyüyüp küçülmesi, kartın açılması, içerik geçişleri"), "animasyon hız yavaş hızlı kapalı"),
    e(0, "marquee", n_("Hareket"), n_("Kayan başlık"), n_("Pile sığmayan şarkı adı"), "kayma marquee başlık"),
    e(0, "marquee-speed", n_("Hareket"), n_("Kayma hızı"), n_("Kayan başlığın hızı"), "kayma hız marquee"),
    e(1, "language", n_("Dil"), n_("Arayüzün dili"), n_("Otomatik: Windows'unki; Türkçe ya da İngilizce"), "dil language english ingilizce türkçe turkish arayüz"),
    e(1, "autostart", n_("Başlangıç"), n_("Windows ile başlat"), n_("Oturum açınca Now Bar kendiliğinden çalışır"), "başlangıç açılış otomatik"),
    e(1, "hotkey", n_("Kısayol ve tepsi"), n_("Seçici kısayolu"), n_("Zamanlayıcı seçicisini açar"), "klavye tuş hotkey win alt"),
    e(1, "question-hotkey", n_("Kısayol ve tepsi"), n_("Claude'a sor"), n_("Kartta bir yazı kutusu açar, cevap Claude Code'dan akar; pile Ctrl ile tıklamak da açar"), "claude soru sor yapay zeka ai kısayol spotlight"),
    e(1, "tray-click", n_("Kısayol ve tepsi"), n_("Tepsi simgesine sol tık"), n_("Zamanlayıcı seçicisi ya da ayarlar"), "tepsi simge tray"),
    e(1, "close-on-leave", n_("Kart ve etkileşim"), n_("Fare ayrılınca kartı kapat"), n_("Fare karttan uzaklaşınca kart kapanır"), "kapat fare"),
    e(1, "leave-ms", n_("Kart ve etkileşim"), n_("Kapanma gecikmesi"), n_("Fare ayrıldıktan sonra kartın kapanma süresi"), "gecikme süre kapanma"),
    e(1, "wheel-switches", n_("Kart ve etkileşim"), n_("Tekerlek ve kaydırma etkinlik değiştirir"), n_("Pilin ve kartın üstünde; dokunmatik yüzeyde iki parmakla"), "tekerlek kaydırma fare touchpad"),
    e(1, "keep-user-front", n_("Kart ve etkileşim"), n_("Seçtiğim etkinlik önde kalsın"), n_("Tekerlekle öne aldığın etkinlik yeni bir şey olana dek önde kalır"), "öne al seçim"),
    e(1, "theme", n_("Bu pencere"), n_("Tema"), n_("Pil ve kartlar her zaman koyudur"), "koyu açık sistem tema karanlık"),
    e(2, "media-on", n_("Medya"), n_("Medyayı göster"), n_("Çalan şarkı pilde; tıklayınca medya kartı"), "müzik şarkı spotify youtube"),
    e(2, "pill-buttons", n_("Pil"), n_("Pil düğmeleri"), n_("Başlık, düğmelere kadar uzar"), "önceki sonraki oynat düğme"),
    e(2, "pill-artist", n_("Pil"), n_("Sanatçıyı göster"), n_("Başlığın yanında, soluk"), "sanatçı"),
    e(2, "pill-lyrics", n_("Pil"), n_("Pilde şarkı sözü"), n_("Eşzamanlı söz varken pilde şarkının adı yerine o anki satır kayar"), "söz sözler şarkı sözü lyrics karaoke satır pil"),
    e(2, "media-waveform", n_("Pil"), n_("Ses dalgası"), n_("Kapakla başlık arasındaki çubuklar sesle birlikte oynar"), "dalga dalgası çubuk ses seviye ekolayzır equalizer"),
    e(2, "card-wave", n_("Kart"), n_("Dalgalı ilerleme çubuğu"), n_("Çalarken dalgalanır; kapalıyken düz çizgi"), "dalga ilerleme çubuk"),
    e(2, "media-lyrics", n_("Kart"), n_("Şarkı sözleri"), n_("Kartta «Sözler» çipi; eşzamanlı sözler lrclib.net'ten gelir"), "söz sözler şarkı sözü lyrics lrclib karaoke"),
    e(2, "middle-click", n_("Davranış"), n_("Orta tıkla oynat/durdur"), n_("Medya pilinin üstünde farenin orta tuşu"), "orta tık fare duraklat"),
    e(2, "paused-keep", n_("Davranış"), n_("Duraklatınca pilde kalma süresi"), n_("Sonra pil kaybolur; çalınca geri gelir"), "duraklat kalma süre"),
    e(2, "apps", n_("Uygulamalar"), n_("Uygulamalar"), n_("Pilde gösterilecek medya uygulamaları"), "spotify brave chrome gizle uygulama"),
    e(2, "devices", n_("Ses çıkışı seçicisi"), n_("Ses çıkışı seçicisi"), n_("Medya kartındaki çıkış sayfasında görünen cihazlar"), "hoparlör kulaklık çıkış cihaz ses"),
    e(3, "timer-on", n_("Zamanlayıcı"), n_("Zamanlayıcı"), n_("Geri sayım; süre dolunca alarm"), "geri sayım timer"),
    e(3, "stopwatch-on", n_("Zamanlayıcı"), n_("Kronometre"), n_("İleri sayım ve tur"), "kronometre stopwatch tur"),
    e(3, "sound", n_("Süre dolunca"), n_("Alarm sesi"), n_("Windows'un alarm ve zil sesleri"), "ses alarm zil"),
    e(3, "done-card", n_("Süre dolunca"), n_("Kartı aç"), n_("Süre dolunca kart kendiliğinden açılır"), "kart açılış bitiş"),
    e(3, "add-minutes", n_("Süre dolunca"), n_("Ekle düğmesi"), n_("Biten zamanlayıcının kartında"), "dakika ekle"),
    e(3, "presets", n_("Seçici"), n_("Hazır süreler"), n_("Seçicideki çipler, dakika olarak (1–180)"), "hazır süre dakika ön ayar"),
    e(3, "tenths", n_("Kronometre"), n_("Saliseyi göster"), n_("Kartta 00:12,4 gibi; kapalıyken saniyede bir güncellenir"), "salise onda"),
    e(4, "buds-on", n_("Galaxy Buds"), n_("Buds'ı göster"), n_("Kulaklığın pili; bağlanınca ve pil azalınca pilde"), "kulaklık pil buds samsung"),
    e(4, "buds-device", n_("Etkinlik"), n_("Kulaklık"), n_("Hangi Buds: bağlanan ilki ya da seçilen eşleşmiş cihaz"), "cihaz seç eşleşmiş bluetooth buds pro adı"),
    e(4, "buds-low", n_("Pil uyarısı"), n_("Düşük pil uyarısı"), n_("Bunun altında Buds öne gelir, halkalar kırmızı olur"), "düşük pil uyarı"),
    e(4, "buds-front", n_("Pilde"), n_("Bağlanınca öne gel"), n_("Kulaklık bağlanınca pil kısa süre önde"), "bağlantı öne"),
    e(4, "buds-case", n_("Pilde"), n_("Kutu pilini göster"), n_("Bir kulaklık kutudayken"), "kutu şarj"),
    e(4, "buds-ear-pause", n_("Müzik"), n_("Kulaktan çıkınca duraklat"), n_("Bir kulaklık çıkınca müzik durur, geri takınca devam eder"), "kulak çıkar tak duraklat devam müzik otomatik"),
    e(4, "buds-offer", n_("Bağlantı"), n_("Kutu açılınca bağlanmayı öner"), n_("Bağlı değilken pilde sorar"), "kutu kapak aç bağlan öner bağlantı telefon"),
    e(5, "match-on", n_("Canlı maç"), n_("Canlı maçı göster"), n_("Yalnızca takip ettiğin takımlar"), "futbol maç skor"),
    e(5, "team-add", n_("Takım ekle"), n_("Takım ekle"), n_("Sonuçlar ESPN'den gelir"), "takım ara fenerbahçe galatasaray"),
    e(5, "teams", n_("Takip edilenler"), n_("Takip edilen takımlar"), n_("Takip ettiğin takımlar"), "takım takip"),
    e(5, "show-before", n_("Ne zaman"), n_("Maçtan önce göster"), n_("Pilde başlama saatiyle"), "önce başlama saat"),
    e(5, "keep-after", n_("Ne zaman"), n_("Maç bitince kalma süresi"), n_("Maç sonu pilde kalma süresi"), "bitiş sonra"),
    e(5, "opponent-moment", n_("Anlar"), n_("Rakip golünde de an göster"), n_("Pil kısa süre büyür, parıltı olmadan"), "gol rakip an"),
    e(5, "goal-sound", n_("Anlar"), n_("Gol sesi"), n_("Takımın gol atınca kısa bir ses çalar"), "gol ses tada"),
    e(6, "fullscreen-hide", n_("Tam ekran"), n_("Tam ekranda gizle"), n_("Pil ve kartlar, tam ekran bir uygulama öndeyken gizlenir"), "oyun video sunum gizle"),
    e(6, "exceptions", n_("İstisnalar"), n_("İstisnalar"), n_("Bu uygulamalar tam ekran olsa da Now Bar görünür kalır"), "istisna uygulama exe"),
    e(7, "update", n_("Güncelleme"), n_("Güncelle"), n_("Yeni sürüm varsa indirir, doğrular ve Now Bar'ı yeniden başlatır"), "güncelle güncelleme sürüm versiyon yeni indir github"),
    e(7, "update-check", n_("Güncelleme"), n_("Güncellemeleri denetle"), n_("Günde bir kez GitHub'a yeni sürüm var mı diye sorar"), "güncelleme denetle otomatik github internet"),
    e(7, "log", n_("Dosyalar"), n_("Günlük"), n_("Now Bar'ın günlük dosyası"), "log kayıt"),
    e(7, "file", n_("Dosyalar"), n_("Ayar dosyası"), n_("settings.json"), "json dosya"),
    e(7, "reset", n_("Sıfırla"), n_("Varsayılanlara dön"), n_("Takımlar ve listeler de sıfırlanır"), "sıfırla varsayılan reset"),
    e(8, "volume-moment", n_("Ses seviyesi"), n_("Ses seviyesini göster"), n_("Ses değişince seviye kısa bir an pilde görünür"), "ses seviye volume yükselt kıs sessiz hoparlör"),
    e(8, "power-moment", n_("Şarj"), n_("Şarjı göster"), n_("Şarj aleti takılınca ve pil azalınca pil kısa bir an öne gelir"), "şarj pil batarya güç adaptör priz dizüstü laptop"),
    e(8, "power-low", n_("Şarj"), n_("Düşük pil"), n_("Dizüstünün pili bunun altına inince bir kez uyarır"), "düşük pil batarya uyarı yüzde"),
    e(8, "privacy-on", n_("Mikrofon / kamera"), n_("Mikrofon ve kamera kullanımı"), n_("Bir uygulama mikrofonu ya da kamerayı kullanırken pilde, uygulamanın adıyla"), "mikrofon kamera gizlilik webcam kayıt discord zoom"),
    e(8, "live-on", n_("Canlı etkinlikler"), n_("Canlı etkinlikler"), n_("nowbar.exe push ve Claude Code'un hook'ları pilde bir etkinlik gösterir"), "claude betik script canlı etkinlik hook push derleme"),
    e(8, "capture-moment", n_("Ekran görüntüleri"), n_("Ekran görüntüsünü göster"), n_("ShareX ya da Ekran Alıntısı Aracı yeni bir görüntü kaydedince pilde kısa bir an; sürükleyip atabilirsin"), "ekran görüntüsü screenshot sharex alıntı snipping kırpma yakalama"),
    e(8, "transfer-on", n_("Dosya kopyalama"), n_("Kopyalama ilerlemesi"), n_("Explorer kopyalar, taşır ya da silerken pilde yüzde ve kalan süre; penceresi küçültülse de"), "kopyala kopyalama taşı sil dosya ilerleme explorer gezgin yüzde kalan süre copy"),
    e(8, "capture-to-shelf", n_("Ekran görüntüleri"), n_("Ekran görüntülerini rafa ekle"), n_("Yeni görüntüler rafa düşer, sonra sürükleyip atmak için"), "ekran görüntüsü raf shelf sharex screenshot"),
    e(8, "claude-ask", n_("Canlı etkinlikler"), n_("Claude izinleri"), n_("Claude izin isteyince kartında İzin ver / Reddet çıkar"), "claude izin onay reddet permission hook"),
    e(8, "claude-ask-wait", n_("Canlı etkinlikler"), n_("İzin için bekleme"), n_("Kart bu kadar cevap bekler, sonra Claude kendi penceresinde sorar"), "claude izin bekleme süre saniye zaman aşımı"),
    e(8, "api-on", n_("Uygulama bildirimleri"), n_("Uygulama bildirimleri"), n_("Diğer uygulamalar 127.0.0.1:20034 üzerinden pilde bildirim gösterebilir; önce sana sorulur"), "api bildirim uygulama izni notification http localhost 20034 geliştirici developer docs"),
    e(8, "api-apps", n_("Uygulama bildirimleri"), n_("İzin verilen uygulamalar"), n_("Sorduklarında verdiğin cevaplar; tıkla: izin ver / engelle"), "api uygulama izin engelle sustur liste"),
    e(8, "headsets-on", n_("Diğer kulaklıklar"), n_("Diğer kulaklıkları göster"), n_("Galaxy Buds dışındaki Bluetooth kulaklıklar, pilleriyle"), "kulaklık bluetooth pil sony airpods jbl"),
    e(8, "brief-morning", n_("Now Brief"), n_("Sabah Now Brief"), n_("Günün ilk kilit açılışında günaydın kartı: hava, maçlar, müzik"), "brief now brief sabah özet ozet günaydın gunaydin kart kilit"),
    e(8, "brief-name", n_("Now Brief"), n_("Selamlamadaki adın"), n_("Boşsa Windows hesabındaki ad"), "ad adın isim selamlama günaydın brief"),
    e(8, "away-summary", n_("Now Brief"), n_("Sen yokken"), n_("Kilidi açınca, kilitliyken olanlar pilde bir an ve bir listede: Claude, goller, yağmur, indirmeler"), "sen yokken kilit kaçırdım özet ne oldu claude gol yağmur indirme"),
    e(8, "idle-clock", n_("Saat ve hava"), n_("Boşta saat ve hava"), n_("Etkinlik yokken pil gizlenmez; saati, tarihi ve havayı gösterir"), "saat tarih hava durumu boşta idle clock"),
    e(8, "weather-in-stack", n_("Saat ve hava"), n_("Havayı hep göster"), n_("Başka etkinlikler varken hava yığının en arkasında durur"), "hava durumu yığın arka her zaman"),
    e(8, "weather-effects", n_("Saat ve hava"), n_("Hava efektleri"), n_("Kar, yağmur, güneş ve yıldızlar havanın arkasında hareket eder"), "efekt animasyon kar yağmur güneş yıldız fırtına sis bulut"),
    e(8, "rain-moment", n_("Saat ve hava"), n_("Yağmur uyarısı"), n_("Şehrinde yağmur ya da kar başlamadan önce pil bir an öne gelir"), "yağmur kar yağış uyarı şemsiye hava"),
    e(8, "weather-alerts", n_("Saat ve hava"), n_("Hava uyarıları"), n_("Fırtına, kuvvetli rüzgâr, aşırı sıcak, don ve kötü hava kalitesi öncesinde pil bir an öne gelir"), "fırtına şimşek gök gürültüsü rüzgar rüzgâr fırtına sıcak sıcaklık don buz kirlilik hava kalitesi aqi uyarı"),
    e(9, "games-downloads", n_("Pilde"), n_("İndirmeler"), n_("Steam ve Epic'te ilerleme, hız ve duraklatma; bitince Güncellendi"), "indirme güncelleme güncellendi steam epic oyun download"),
    e(9, "games-playtime", n_("Pilde"), n_("Oyun süresi"), n_("Oyun açıkken pilde; kapanınca ne kadar oynadığın"), "oyun süre oynama oynadın fortnite steam epic"),
    e(9, "island-on", n_("Tam ekran"), n_("Oyun içi ada"), n_("Tam ekranda gol, Claude'un izni, biten zamanlayıcı gibi acil anlar ekranın üstünde birkaç saniye; tıklamayı engellemez"), "oyun içi ada tam ekran üst bildirim gol claude zamanlayıcı dynamic island"),
    e(9, "games-steam", n_("Mağazalar"), n_("Steam"), n_("Steam'in kayıt defteri ve kütüphane dosyaları"), "steam valve oyun kütüphane"),
    e(9, "games-epic", n_("Mağazalar"), n_("Epic Games"), n_("Yeni Epic launcher'ın klasörü"), "epic games launcher fortnite oyun"),
    e(10, "wellbeing-on", n_("Dijital Denge"), n_("Ekran süresi"), n_("Hangi uygulamanın ne kadar önde kaldığı; bu bilgisayarda kalır"), "ekran süresi dijital denge kullanım uygulama süre screen time"),
    e(10, "wellbeing-in-brief", n_("Dijital Denge"), n_("Now Brief'te göster"), n_("Brief'te dünün ya da bugünün ekran süresi"), "brief ekran süresi sabah"),
    e(10, "wellbeing-limits", n_("Sınırlar"), n_("Uygulama sınırları"), n_("Bir uygulama günlük sınırını geçince pilde bir an"), "sınır limit süre uyarı discord oyun mola"),
    e(10, "music-week", n_("Müzik"), n_("Haftanın müziği"), n_("En çok dinlenen şarkılar ve sanatçılar; çalanlar bu bilgisayarda sayılır, internete gitmez"), "müzik şarkı sanatçı haftalık özet wrapped spotify en çok dinlenen"),
    e(8, "weather-locate", n_("Saat ve hava"), n_("Konumumu kullan"), n_("Şehir seçilmemişse hava bilgisayarın bulunduğu yerin; konum Windows'tan, yer adı OpenStreetMap'ten"), "konum otomatik yer neredeyim gps location"),
    e(8, "weather-city", n_("Saat ve hava"), n_("Hava için şehir"), n_("Sıcaklık ve gökyüzü bu şehrin; veriler Open-Meteo'dan gelir"), "şehir il konum hava durumu sıcaklık derece weather"),
];

/// Lower case without Turkish marks, one character for one (so match positions carry over):
/// İ, I, ı → i; ş → s, ç → c, ğ → g, ö → o, ü → u, â/î/û → a/i/u.
pub fn fold(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'İ' | 'I' | 'ı' | 'î' | 'Î' => 'i',
            'Ş' | 'ş' => 's',
            'Ç' | 'ç' => 'c',
            'Ğ' | 'ğ' => 'g',
            'Ö' | 'ö' => 'o',
            'Ü' | 'ü' | 'û' | 'Û' => 'u',
            'Â' | 'â' => 'a',
            c => c.to_lowercase().next().unwrap_or(c),
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq)]
pub struct SearchHit {
    pub page: u8,
    pub row: &'static str,
    pub section: &'static str,
    pub title: &'static str,
    /// "Görünüm › Pil".
    pub path: String,
    pub desc: &'static str,
    /// The matched part of the title, in characters (0, 0 when the match was elsewhere).
    pub mark_start: usize,
    pub mark_len: usize,
}

/// Rows matching `query`: title starting with it first, then title containing it, then
/// description, section, page or keywords; ties keep page order.
pub fn search(query: &str) -> Vec<SearchHit> {
    let q = fold(query.trim());
    if q.is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<(u8, SearchHit)> = INDEX
        .iter()
        .filter_map(|x| {
            // What the row shows is in the interface's language; its Turkish words find it too.
            let (shown, page) = (tr(x.title), tr(PAGES[x.page as usize]));
            let title = fold(shown);
            let at = title.find(&q).map(|byte| title[..byte].chars().count());
            let others = [tr(x.desc), tr(x.section), page, x.title, x.desc, x.section, PAGES[x.page as usize], x.keywords];
            let score = match at {
                Some(0) => 3,
                Some(_) => 2,
                None if others.iter().any(|t| fold(t).contains(&q)) => 1,
                None => return None,
            };
            let (mark_start, mark_len) = at.map_or((0, 0), |i| (i, q.chars().count()));
            let path = format!("{page} › {}", tr(x.section));
            Some((score, SearchHit { page: x.page, row: x.row, section: tr(x.section), title: shown, path, desc: tr(x.desc), mark_start, mark_len }))
        })
        .collect();
    scored.sort_by_key(|(score, h)| (std::cmp::Reverse(*score), h.page));
    scored.into_iter().map(|(_, h)| h).collect()
}

/// A fullscreen exception as typed: trimmed, ".exe" added. `Ok(None)` for nothing typed;
/// `Err` with the message the window shows.
pub fn exception_name(text: &str, existing: &[String]) -> Result<Option<String>, String> {
    let name = text.trim();
    if name.is_empty() {
        return Ok(None);
    }
    if name.chars().any(|c| matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|')) {
        return Err(tr("Geçersiz ad").into());
    }
    let exe = if name.to_lowercase().ends_with(".exe") { name.to_string() } else { format!("{name}.exe") };
    match existing.iter().find(|x| x.eq_ignore_ascii_case(&exe)) {
        Some(have) => Err(trf("{} zaten listede", &[&have])),
        None => Ok(Some(exe)),
    }
}

/// The picker's presets as saved: 1–180 minutes, no repeats, ascending, at most six (v1's six
/// when nothing valid is left).
pub fn tidy_presets(v: &[i32]) -> Vec<u32> {
    let mut m: Vec<u32> = v.iter().filter(|&&x| (1..=180).contains(&x)).map(|&x| x as u32).collect();
    m.sort_unstable();
    m.dedup();
    m.truncate(6);
    if m.is_empty() {
        vec![1, 3, 5, 10, 15, 30]
    } else {
        m
    }
}

/// The letters on a crest stand-in: ESPN's code, else the initials of two words, else the first
/// three letters.
pub fn team_code(name: &str, abbr: &str) -> String {
    if !abbr.trim().is_empty() {
        return abbr.trim().to_string();
    }
    let words: Vec<&str> = name.split_whitespace().collect();
    let code: String = if words.len() >= 2 { words.iter().take(2).filter_map(|w| w.chars().next()).collect() } else { name.chars().take(3).collect() };
    code.to_uppercase()
}

/// The highest settings page.
pub const LAST_PAGE: u8 = PAGES.len() as u8 - 1;

/// `--page N` on the settings window's command line: the page to open on.
pub fn page_arg(args: &[String]) -> Option<u8> {
    let i = args.iter().position(|a| a == "--page")?;
    args.get(i + 1)?.parse::<u8>().ok().filter(|p| *p <= LAST_PAGE)
}

/// `--at x,y,w,h` on the settings window's command line: the window rect (physical px, as
/// `GetWindowRect` gives it) a window restarted by an update reopens at.
pub fn at_arg(args: &[String]) -> Option<(i32, i32, u32, u32)> {
    let i = args.iter().position(|a| a == "--at")?;
    let parts: Vec<i32> = args.get(i + 1)?.split(',').map(|p| p.trim().parse().ok()).collect::<Option<_>>()?;
    match parts[..] {
        [x, y, w, h] if w > 0 && h > 0 => Some((x, y, w as u32, h as u32)),
        _ => None,
    }
}

/// The letter on a media app's badge: its name's first letter or digit, upper-cased.
pub fn badge_letter(name: &str) -> String {
    name.chars().find(|c| c.is_alphanumeric()).map_or_else(|| "?".to_string(), |c| c.to_uppercase().collect())
}

/// The badge colour of a media app in the settings list (notes §2.1).
pub fn app_badge(app_id: &str) -> [u8; 3] {
    let id = app_id.to_ascii_lowercase();
    let table: [(&str, [u8; 3]); 8] = [
        ("spotify", [0x1D, 0xB9, 0x54]),
        ("youtube", [0xFF, 0x00, 0x33]),
        ("brave", [0xFB, 0x54, 0x2B]),
        ("discord", [0x58, 0x65, 0xF2]),
        ("chrome", [0x42, 0x85, 0xF4]),
        ("msedge", [0x0C, 0x59, 0xA4]),
        ("firefox", [0xFF, 0x71, 0x39]),
        // Firefox's media sessions come with this id.
        ("308046b0af4a39cb", [0xFF, 0x71, 0x39]),
    ];
    table.iter().find(|(k, _)| id.contains(k)).map_or([0x8A, 0x93, 0xB4], |(_, c)| *c)
}

/// A steady, never-dark colour for a team without colours (from its id).
pub fn stand_in_colour(id: &str) -> [u8; 3] {
    let hash = id.bytes().fold(0x811C_9DC5u32, |h, b| (h ^ b as u32).wrapping_mul(0x0100_0193));
    let hue = (hash % 360) as f32;
    let (s, v) = (0.55f32, 0.78f32);
    let c = v * s;
    let x = c * (1.0 - ((hue / 60.0) % 2.0 - 1.0).abs());
    let (r, g, b) = match (hue / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    [((r + m) * 255.0) as u8, ((g + m) * 255.0) as u8, ((b + m) * 255.0) as u8]
}

/// The Galaxy Buds page's device list: its labels, what each stands for (`None`: automatic) and
/// the one picked now.
#[derive(Debug, PartialEq)]
pub struct BudsChoices {
    pub labels: Vec<String>,
    pub values: Vec<Option<BudsDevice>>,
    pub current: usize,
}

/// "Otomatik", then the paired candidates (Samsung's service first; a name two of them share
/// gets its address's end), then the chosen pair when it's no longer paired.
pub fn buds_choices(paired: &[Paired], chosen: Option<&BudsDevice>) -> BudsChoices {
    let mut order: Vec<&Paired> = paired.iter().collect();
    order.sort_by_key(|p| !p.samsung);
    let mut labels = vec![tr("Otomatik").to_string()];
    let mut values = vec![None];
    for p in &order {
        let name = if p.name.trim().is_empty() { tr("Adsız cihaz") } else { p.name.trim() };
        let shared = order.iter().filter(|o| o.name.trim() == p.name.trim()).count() > 1;
        let end = &p.address[p.address.len().saturating_sub(5)..];
        labels.push(if shared { format!("{name} · {end}") } else { name.to_string() });
        values.push(Some(BudsDevice { address: p.address.clone(), name: p.name.clone() }));
    }
    let at = |c: &BudsDevice| values.iter().position(|v| v.as_ref().is_some_and(|v| v.address.eq_ignore_ascii_case(&c.address)));
    let current = match chosen {
        None => 0,
        Some(c) => match at(c) {
            Some(i) => i,
            None => {
                labels.push(trf("{} · eşleşmemiş", &[&c.name]));
                values.push(Some(c.clone()));
                values.len() - 1
            }
        },
    };
    BudsChoices { labels, values, current }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_speaks_the_interfaces_language() {
        use crate::i18n::{in_language, Language};
        in_language(Language::English, || {
            let hits = search("opacity");
            assert!(hits.iter().any(|h| h.row == "pill-opacity" && h.title == "Background opacity"), "an English word finds the row");
            assert!(search("opaklığı").iter().any(|h| h.row == "pill-opacity"), "and the Turkish still does");
            let h = search("opacity").into_iter().find(|h| h.row == "pill-opacity").unwrap();
            assert_eq!((h.mark_start, h.mark_len), (11, 7), "the mark is on the English title");
            assert!(h.path.starts_with("Appearance › "), "{}", h.path);
        });
    }

    #[test]
    fn buds_choices_start_with_automatic_and_keep_a_lost_pick() {
        let p = |address: &str, name: &str, samsung| Paired { address: address.into(), name: name.into(), samsung };
        let all = [p("02:00:00:00:00:03", "HC-05", false), p("02:00:00:00:00:01", "Galaxy Buds2", true), p("02:00:00:00:00:02", "Galaxy Buds2", true), p("02:00:00:00:00:04", "", false)];
        let c = buds_choices(&all, None);
        assert_eq!(c.labels, ["Otomatik", "Galaxy Buds2 · 00:01", "Galaxy Buds2 · 00:02", "HC-05", "Adsız cihaz"]);
        assert_eq!((c.current, &c.values[0]), (0, &None));
        assert_eq!(c.values[3], Some(BudsDevice { address: "02:00:00:00:00:03".into(), name: "HC-05".into() }));
        let chosen = BudsDevice { address: "02:00:00:00:00:02".into(), name: "Galaxy Buds2".into() };
        assert_eq!(buds_choices(&all, Some(&chosen)).current, 2);
        let lost = BudsDevice { address: "02:00:00:00:00:09".into(), name: "Eski Buds".into() };
        let c = buds_choices(&all, Some(&lost));
        assert_eq!((c.labels.last().map(String::as_str), c.current, c.values.last()), (Some("Eski Buds · eşleşmemiş"), 5, Some(&Some(lost.clone()))));
    }

    #[test]
    fn search_finds_the_app_api() {
        for q in ["api", "bildirim", "20034", "uygulama izni"] {
            assert!(search(q).iter().any(|h| h.row == "api-on" && h.page == 8), "{q}");
        }
        assert!(search("izin verilen").iter().any(|h| h.row == "api-apps"));
        crate::i18n::in_language(crate::i18n::Language::English, || {
            assert!(search("notifications").iter().any(|h| h.row == "api-on" && h.title == "App notifications"));
        });
    }

    #[test]
    fn folding_ignores_turkish_letters_and_case() {
        assert_eq!(fold("OPAKLIK"), "opaklik");
        assert_eq!(fold("İstisna Şerit Çıkış Ğ Ö Ü"), "istisna serit cikis g o u");
        assert_eq!(fold("bulanık"), fold("BULANIK"));
    }

    #[test]
    fn search_finds_rows_by_title_description_and_keywords() {
        let hits = search("opak");
        assert_eq!(hits[0].title, "Zemin opaklığı");
        assert_eq!(hits[0].page, 0);
        assert_eq!((hits[0].mark_start, hits[0].mark_len), (6, 4));
        // A keyword only ("blur") still finds the row.
        assert!(search("blur").iter().any(|h| h.title == "Bulanıklık"));
        // Accent-insensitive: "kisayol" finds "Seçici kısayolu".
        assert!(search("kisayol").iter().any(|h| h.title == "Seçici kısayolu"));
        assert!(search("").is_empty());
        assert!(search("zzzz").is_empty());
    }

    #[test]
    fn search_finds_the_games_and_screen_time() {
        let rows = |q: &str, page: u8| search(q).into_iter().filter(|h| h.page == page).map(|h| h.row).collect::<Vec<_>>();
        assert_eq!((PAGES[9], PAGES[10]), ("Oyunlar", "Dijital Denge"));
        assert!(rows("steam", 9).contains(&"games-steam"), "{:?}", rows("steam", 9));
        assert!(rows("fortnite", 9).contains(&"games-epic"), "{:?}", rows("fortnite", 9));
        assert!(rows("indirme", 9).contains(&"games-downloads"), "{:?}", rows("indirme", 9));
        assert!(rows("ada", 9).contains(&"island-on"), "{:?}", rows("ada", 9));
        assert!(rows("tam ekran", 9).contains(&"island-on"), "{:?}", rows("tam ekran", 9));
        assert!(rows("ekran süresi", 10).contains(&"wellbeing-on"), "{:?}", rows("ekran süresi", 10));
        assert!(rows("sınır", 10).contains(&"wellbeing-limits"), "{:?}", rows("sınır", 10));
        assert_eq!(LAST_PAGE, 10);
    }

    #[test]
    fn search_finds_the_wallpaper_accent() {
        for q in ["duvar", "duvar kağıdı", "palet", "arka plan"] {
            assert!(search(q).iter().any(|h| h.row == "wallpaper-accent" && h.page == 0), "{q}");
        }
    }

    #[test]
    fn search_ranks_title_starts_first() {
        let hits = search("kart");
        assert!(hits[0].title.to_lowercase().starts_with("kart"), "{:?}", hits[0].title);
        assert_eq!(hits[0].path, format!("{} › {}", PAGES[hits[0].page as usize], hits[0].section));
    }

    #[test]
    fn search_finds_the_system_settings() {
        let rows = |q: &str, page: u8| search(q).into_iter().filter(|h| h.page == page).map(|h| h.row).collect::<Vec<_>>();
        assert_eq!(PAGES.get(8), Some(&"Sistem"));
        assert!(rows("şarj", 8).contains(&"power-moment"), "{:?}", rows("şarj", 8));
        assert!(rows("pil", 8).contains(&"power-low"), "{:?}", rows("pil", 8));
        assert!(rows("ses", 8).contains(&"volume-moment"), "{:?}", rows("ses", 8));
        assert!(rows("mikrofon", 8).contains(&"privacy-on"), "{:?}", rows("mikrofon", 8));
        assert!(rows("kamera", 8).contains(&"privacy-on"), "{:?}", rows("kamera", 8));
        assert!(rows("kulaklık", 8).contains(&"headsets-on"), "{:?}", rows("kulaklık", 8));
        assert!(rows("claude", 8).contains(&"live-on"), "{:?}", rows("claude", 8));
        assert!(rows("betik", 8).contains(&"live-on"), "{:?}", rows("betik", 8));
        assert!(rows("söz", 2).contains(&"media-lyrics"), "{:?}", rows("söz", 2));
        assert!(rows("dalga", 2).contains(&"media-waveform"), "{:?}", rows("dalga", 2));
        // Every entry's page has a name (the hit's path is "Sistem › Şarj").
        assert!(INDEX.iter().all(|x| (x.page as usize) < PAGES.len()));
        assert_eq!(search("mikrofon")[0].path, "Sistem › Mikrofon / kamera");
    }

    #[test]
    fn search_finds_the_brief() {
        let rows = |q: &str| search(q).into_iter().filter(|h| h.page == 8).map(|h| h.row).collect::<Vec<_>>();
        for q in ["brief", "sabah", "günaydın", "ozet"] {
            assert!(rows(q).contains(&"brief-morning"), "{q}: {:?}", rows(q));
        }
        assert!(rows("adın").contains(&"brief-name"), "{:?}", rows("adın"));
        assert_eq!(search("günaydın")[0].path, "Sistem › Now Brief");
    }

    #[test]
    fn search_finds_the_clock_and_weather() {
        let rows = |q: &str| search(q).into_iter().filter(|h| h.page == 8).map(|h| h.row).collect::<Vec<_>>();
        assert!(rows("saat").contains(&"idle-clock"), "{:?}", rows("saat"));
        assert!(rows("hava").contains(&"idle-clock"), "{:?}", rows("hava"));
        assert!(rows("şehir").contains(&"weather-city"), "{:?}", rows("şehir"));
        assert!(rows("sehir").contains(&"weather-city"), "{:?}", rows("sehir"));
        assert!(rows("sıcaklık").contains(&"weather-city"), "{:?}", rows("sıcaklık"));
        assert!(rows("yağmur").contains(&"rain-moment"), "{:?}", rows("yağmur"));
        assert!(rows("şemsiye").contains(&"rain-moment"), "{:?}", rows("şemsiye"));
        assert_eq!(search("şehir")[0].path, "Sistem › Saat ve hava");
        assert!(rows("efekt").contains(&"weather-effects"), "{:?}", rows("efekt"));
        for q in ["fırtına", "rüzgar", "don", "hava kalitesi", "uyarı"] {
            assert!(rows(q).contains(&"weather-alerts"), "{q}: {:?}", rows(q));
        }
        assert!(rows("kar").contains(&"weather-effects"), "{:?}", rows("kar"));
        assert!(rows("hava").contains(&"weather-in-stack"), "{:?}", rows("hava"));
    }

    #[test]
    fn exception_names_are_checked_and_get_exe() {
        let have = vec!["vlc.exe".to_string()];
        assert_eq!(exception_name("  mpv ", &have), Ok(Some("mpv.exe".into())));
        assert_eq!(exception_name("Game.EXE", &have), Ok(Some("Game.EXE".into())));
        assert_eq!(exception_name("   ", &have), Ok(None));
        assert_eq!(exception_name("VLC", &have), Err("vlc.exe zaten listede".into()));
        assert_eq!(exception_name("a/b.exe", &have), Err("Geçersiz ad".into()));
    }

    #[test]
    fn presets_are_sorted_unique_in_range_and_at_most_six() {
        assert_eq!(tidy_presets(&[30, 5, 5, 0, 200, 1, 10, 15, 3, 45]), vec![1, 3, 5, 10, 15, 30]);
        assert_eq!(tidy_presets(&[]), vec![1, 3, 5, 10, 15, 30]);
        assert_eq!(tidy_presets(&[90, 7]), vec![7, 90]);
    }

    #[test]
    fn page_arg_parses() {
        let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(page_arg(&args(&["nowbar-settings.exe", "--page", "5"])), Some(5));
        assert_eq!(page_arg(&args(&["nowbar-settings.exe"])), None);
        assert_eq!(page_arg(&args(&["x", "--page", &(LAST_PAGE + 1).to_string()])), None, "no such page");
        assert_eq!(page_arg(&args(&["x", "--page", "ekin"])), None);
        assert_eq!(page_arg(&args(&["x", "--page"])), None);
    }

    #[test]
    fn at_arg_parses_a_window_rect() {
        let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(at_arg(&args(&["x", "--page", "7", "--at", "-1913,96,1408,1012"])), Some((-1913, 96, 1408, 1012)), "a monitor left of the main one");
        assert_eq!(at_arg(&args(&["x"])), None);
        assert_eq!(at_arg(&args(&["x", "--at", "10,20,0,500"])), None, "no size");
        assert_eq!(at_arg(&args(&["x", "--at", "10,20,30"])), None);
        assert_eq!(at_arg(&args(&["x", "--at", "10,20,30,40,50"])), None);
        assert_eq!(at_arg(&args(&["x", "--at", "a,b,c,d"])), None);
        assert_eq!(at_arg(&args(&["x", "--at"])), None);
    }

    #[test]
    fn badge_letter_is_the_first_letter_or_digit() {
        assert_eq!(badge_letter("Spotify"), "S");
        assert_eq!(badge_letter("youTube Music"), "Y");
        assert_eq!(badge_letter("  (beta) Brave"), "B");
        assert_eq!(badge_letter("7-Zip"), "7");
        assert_eq!(badge_letter("şarkıcı"), "Ş");
        assert_eq!(badge_letter(""), "?");
    }

    #[test]
    fn app_badges_by_app() {
        assert_eq!(app_badge("Spotify.exe"), [0x1D, 0xB9, 0x54]);
        assert_eq!(app_badge("com.github.th-ch.youtube-music"), [0xFF, 0x00, 0x33]);
        assert_eq!(app_badge("Brave"), [0xFB, 0x54, 0x2B]);
        assert_eq!(app_badge("MSEdge"), [0x0C, 0x59, 0xA4]);
        assert_eq!(app_badge("308046B0AF4A39CB"), [0xFF, 0x71, 0x39]);
        assert_eq!(app_badge("vlc.exe"), [0x8A, 0x93, 0xB4]);
    }

    #[test]
    fn team_stand_in_code_and_colours() {
        assert_eq!(team_code("Real Madrid", ""), "RM");
        assert_eq!(team_code("Liverpool", "LIV"), "LIV");
        assert_eq!(team_code("Beşiktaş", ""), "BEŞ");
        // No colours from ESPN: a steady colour from the id, never black.
        let (a, b) = (stand_in_colour("364"), stand_in_colour("364"));
        assert_eq!(a, b);
        assert_ne!(stand_in_colour("364"), stand_in_colour("102"));
        assert!(a.iter().any(|&c| c > 60));
    }
}
