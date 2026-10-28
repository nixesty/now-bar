<div align="center">

<img src="assets/readme/hero.png" width="880" alt="Görev çubuğunda çalan şarkının pili, üstünde açılmış müzik kartı; yanında Claude Code, maç, zamanlayıcı, Galaxy Buds, hava ve şarj pilleri">

# Now Bar

**Samsung One UI'daki Now Bar, Windows 11 ve 10 görev çubuğunda.**

Görev çubuğunda küçük bir pil durur ve o an olan biteni gösterir: çalan şarkı, zamanlayıcı, maçın<br>
skoru, Claude Code'un ne yaptığı. Pile tıklayınca üstünde bir kart açılır.

**[İndir](https://github.com/nixesty/now-bar/releases/latest)** &nbsp;·&nbsp;
[Neler gösterir](#neler-gösterir) &nbsp;·&nbsp;
[Kurulum](#kurulum) &nbsp;·&nbsp;
[Derleme](#derleme)

<sub>One UI's Now Bar in the Windows 11 and 10 taskbar. The app speaks Turkish and English.</sub>

</div>

<br>

## Neler gösterir

<img src="assets/readme/pills.png" width="880" alt="Pillerden örnekler: müzik, Claude Code izin isteği, maç, zamanlayıcı, hava, yağmur uyarısı, şarj, Galaxy Buds, kamera ve mikrofon, Steam indirmesi, raf, ekran süresi sınırı, saat ve ses">

- **Müzik:** Windows'ta medya oturumu açan her uygulama (Spotify, YouTube Music, tarayıcılar).
  Pil, albüm kapağının rengini alır; kartta kapak, dalgalı ilerleme çubuğu, şarkı sözleri ve ses
  çıkışı seçimi bulunur. Eşzamanlı söz varsa pilde şarkının adı yerine o anki satır kayar.
- **Haftanın müziği:** son yedi günde en çok dinlediğin şarkılar ve sanatçılar; Dijital Denge
  kartından ya da tepsi menüsünden açılır.
- **Zamanlayıcı ve kronometre:** seçiciden (Win+Alt+N) ya da tepsi menüsünden başlatılır; süre
  dolunca alarm çalar ve kart açılır.
- **Canlı maç:** takip ettiğin futbol ve basketbol takımlarının skoru, dakikası ve gol anları.
- **Hava durumu:** boştayken saat ve hava durumu; yağmur, kar, fırtına, kuvvetli rüzgâr, aşırı
  sıcak, don ve kötü hava kalitesinde uyarı. Şehir seçilmemişse konum Windows'tan alınır.
- **Galaxy Buds:** iki kulaklığın ve kutunun şarjı, gürültü kontrolü. Birden fazla Buds
  eşleştirilmişse hangisinin gösterileceği ayarlardan seçilir.
- **Claude Code:** Claude'un o an ne yaptığı ve görev listesi; izin istediğinde kartta
  **İzin ver / Reddet** düğmeleri çıkar ([aşağıda](#canlı-etkinlikler)).
- **Claude'a sor:** Win+Alt+C ya da pile Ctrl ile tıklamak kartta bir yazı kutusu açar; cevap
  Claude Code'dan akar. Bilgisayarda Claude Code kurulu ve oturumu açık olmalıdır (bir kez
  terminalde `claude`, sonra `/login`).
- **Oyunlar:** Steam ve Epic Games indirmeleri, güncellemeleri, duraklatmaları ve oyun süresi.
- **Dijital Denge:** günün ekran süresi, uygulamaların payı ve koyduğun sınırlar.
- **Raf:** pile bıraktığın dosyalar orada bekler, sonra istediğin yere sürükleyebilirsin; yeni
  ekran görüntüleri rafa kendiliğinden eklenir. Kartın altındaki düğmeler seçtiğin dosyalara
  (seçim yoksa hepsine) uygulanır: **Paylaş** (Windows'un paylaşım penceresi; Quick Share
  kuruluysa telefona da), **Metni kopyala** (ekran görüntüsündeki yazıyı panoya alır),
  **Küçült** (resmi JPEG olarak, uzun kenarı en fazla 1920 piksel, aslının yanına kaydeder) ve
  **Zip yap**.
- **Dosya kopyalama:** Explorer kopyalarken, taşırken ya da silerken pilde yüzde ve kalan süre;
  Explorer'ın penceresi küçültülse de.
- **Sen yokken:** bilgisayar en az bir dakika kilitli kaldıysa ve o arada bir şey olduysa, kilidi
  açınca pilde "Sen yokken" belirir; tıklayınca o arada olanların listesi açılır: Claude'un
  bitirdiği işler, goller, yağmur ve hava uyarıları, biten indirmeler, zamanlayıcılar, uygulama
  bildirimleri.
- **Sistem:** ses seviyesi, şarj ve düşük pil uyarısı, mikrofonu ya da kamerayı kullanan
  uygulama, Bluetooth kulaklıkların şarjı.
- Aynı anda birden fazla etkinlik varsa öndekinin sağında ince şeritler görünür; pilin üzerinde
  fare tekerleğini çevirerek aralarında geçiş yapılır. Oyun, video ya da sunum tam ekrandayken pil
  gizlenir; bekleyemeyecek şeyler (Claude'un izin isteği, hava uyarısı) ekranın üstünde küçük bir
  adada belirir.

## Kartlar

<img src="assets/readme/music.png" width="880" alt="Müzik kartı: kapak, ilerleme çubuğu ve düğmeler; yanında aynı şarkının sözleri; altında müzik pili">

Kart pilden büyüyerek açılır, fare uzaklaşınca küçülerek kapanır. Müzik kartında kapağın
köşesindeki **Sözler** şarkı sözlerini açar; sözler `lrclib.net`'ten, orada yoksa NetEase Cloud
Music'ten ya da Kugou'dan gelir ve şarkıyla birlikte akar. Yavaşlatılmış (slowed) ya da
hızlandırılmış bir sürümün sözleri hiçbirinde yoksa orijinalinkiler onun uzunluğuna esnetilir.

<img src="assets/readme/day.png" width="880" alt="Günün özeti kartı, zamanlayıcı kartı, saatlik hava durumu kartı ve Dijital Denge kartı">

Boştayken saate tıklayınca günün özeti açılır: hava, maçlar, son çalan şarkı ve ekran süresi.

<img src="assets/readme/live.png" width="880" alt="Canlı maç kartı, Claude Code izin kartı, Galaxy Buds kartı ve raf kartı">

## Ayarlar

<img src="assets/readme/settings.png" width="880" alt="Ayarlar penceresi: solda sayfalar, ortada pil ve kart boyutunun önizlemeleri, sağda görünüm ayarları">

Ayarlar penceresi tepsi menüsündeki **Ayarlar…** ile açılır; her değişiklik pile anında yansır.
Üstteki arama çubuğu ayarları bulur; Canlı maç sayfasında takım arar, Tam ekran sayfasında istisna
ekler. Ctrl+F aramaya, Ctrl+K eylemler menüsüne götürür. Arayüz Türkçe ve İngilizcedir
(Genel → Dil; varsayılan olarak Windows'un dili).

## Kurulum

<img src="assets/readme/setup.png" width="480" align="right" alt="Kurulum penceresi">

[Son sürümden](https://github.com/nixesty/now-bar/releases/latest) `nowbar-setup.exe`'yi indirip
çalıştır. Yönetici izni istemez ve `%LOCALAPPDATA%\Programs\NowBar` klasörüne kurulur
("Değiştir" ile başka bir klasör seçilebilir). Kurulum ekranındaki seçenekler:

- **Windows'la başlat**
- **Claude Code'a bağlan** (bilgisayarda Claude Code varsa görünür)
- **Uygulama bildirimlerini aç** (varsayılan olarak kapalı): başka uygulamaların pilde bildirim
  göstermesine izin verir ([aşağıda](#canlı-etkinlikler)). İzin verdiğin uygulamalar ve
  gösterdikleri senin sorumluluğundadır.
- **Masaüstü kısayolu**
- **Bitince ayarları aç**

Başlat menüsüne her zaman bir kısayol eklenir. Now Bar, Windows'un "Ayarlar → Uygulamalar"
listesinde görünür ve oradan kaldırılabilir. Kaldırınca kısayollar, Windows'la başlatma ve Claude
Code hook'ları da silinir; ayarları silmek isteğe bağlıdır.

<br clear="right">

Now Bar kuruluyken kurulumu yeniden çalıştırırsan Now Bar güncellenir ya da onarılır: çalışan Now
Bar kapatılır, dosyaları yenilenir ve yeniden başlatılır.

Sessiz kurulum için `nowbar-setup.exe --quiet` (uygulama bildirimleri ayarına dokunmaz), sessiz
kaldırma için `"%LOCALAPPDATA%\Programs\NowBar\nowbar-setup.exe" --uninstall --quiet` kullanılır.

## Kullanım

| Ne | Nasıl |
|---|---|
| Kartı aç / kapat | Pile tıkla; fare uzaklaşınca kart kendiliğinden kapanır |
| Oynat / duraklat | Pile farenin orta tuşuyla tıkla |
| Etkinlikler arasında geçiş | Pilin ya da kartın üzerinde fare tekerleği |
| Zamanlayıcı seçicisi | Win+Alt+N (Ayarlar → Genel'den değiştirilebilir) |
| Claude'a sor | Win+Alt+C ya da pile Ctrl ile tıkla (Ayarlar → Genel'den değiştirilebilir) |
| Raftaki dosyaları seç | Kartta dosyaya tıkla; yeniden tıklayınca seçim kalkar |
| Tepsi menüsü | Now Brief, Dijital Denge, Haftanın müziği, Claude'a sor, Zamanlayıcı başlat, Kronometre başlat, Windows ile başlat, Ayarlar…, Çıkış |

## Canlı etkinlikler

Başka bir program ya da betik pilde bir etkinlik gösterebilir. Now Bar çalışmıyorsa komut hiçbir
şey yapmadan çıkar (çıkış kodu her zaman 0'dır, çıktı vermez):

```
nowbar.exe push --id yedek --title "Yedekleme" --text "Belgeler" --progress 40
nowbar.exe push --id yedek --title "Yedekleme" --state done
```

`--state`: `run` (varsayılan), `wait` (kullanıcıyı bekliyor; kısa bir süre öne gelir), `done`
(5 saniye "Bitti" gösterir), `clear` (hemen kaldırır). Bir saat boyunca güncellenmeyen etkinlik
kendiliğinden kalkar. Bu özellik Ayarlar → Sistem → Canlı etkinlikler'den kapatılabilir.

**Uygulamalar için API:** başka uygulamalar tek bir HTTP isteğiyle pilde bildirim ya da canlı
etkinlik gösterebilir; kendi ikonlarını, renklerini ve en fazla iki düğme kullanabilirler. API'ye
yalnızca bu bilgisayardan (`127.0.0.1:20034`) erişilebilir ve sen açana kadar kapalıdır (kurulumda
**Uygulama bildirimlerini aç** ya da Ayarlar → Sistem → Uygulama bildirimleri). Bir uygulama ilk
kez istek gönderdiğinde pil "Bildirim göstermek istiyor" diye sorar; verdiğin cevap hatırlanır ve
ayarlardan değiştirilebilir. İzin verdiğin uygulamalar ve gösterdikleri senin sorumluluğundadır;
Now Bar yalnızca gösterir. Geliştiriciler için: [DOCS.md](DOCS.md).

```
curl -H "Content-Type: application/json" -d "{\"app\":\"Yedekleyici\",\"title\":\"Yedek bitti\"}" http://127.0.0.1:20034/v1/notify
```

**Claude Code:** kurulumdaki **Claude Code'a bağlan** seçeneği hook'ları kurar. Elle kurmak ya da
kaldırmak için aşağıdaki komutlar kullanılır. Komut exe'nin kendi yolunu yazar, ilk seferde
`settings.json.before-nowbar`, sonraki her yazışta `settings.json.nowbar-last` adıyla yedek alır ve
başka hook'lara dokunmaz:

```
nowbar.exe claude-hooks install
nowbar.exe claude-hooks remove
```

Pil, Claude'un ne yaptığını ve ne kadar süredir çalıştığını gösterir ("now-bar · Düzenliyor:
app.rs"); kartta görev listesinin ilerlemesi, son adımlar ve iş bitince cevabın ilk satırı
("Bitti · 12 dk · 9 dosya") bulunur. Claude izin istediğinde kartta **İzin ver / Reddet**
düğmeleri çıkar; bu yalnızca kartta tamamı görünen tek satırlık komutlar, dosyalar, adresler ve
aramalar için geçerlidir. Dosya izinlerinde kartta dosyanın tam yolu görünür, değişikliğin
kendisi Claude'un penceresinde kalır; çok satırlı komutlar, sorular ve plan onayı da orada
cevaplanır. Claude'un kendi penceresi de aynı anda açıktır; hangisinden önce cevap verilirse o
geçerli olur. Kart cevabı 60 saniye bekler (Ayarlar → Sistem → Canlı etkinlikler). Now Bar başka
bir klasöre taşınırsa hook'lar bir sonraki açılışta yeni yola güncellenir. Masaüstü uygulamasının
Code sekmesinde, CLI'da ve VS Code'da çalışır.

## Derleme

Gerekenler: Rust (MSVC araç zinciri) ve simgenin exe'lere gömülmesi için Windows SDK (`rc.exe`).
SDK olmadan da derlenir, ama exe'ler Windows'un varsayılan simgesiyle kalır.

```
cargo b
```

`cargo b`, `.cargo\config.toml`'da tanımlı bir kısaltmadır: `cargo build --profile quick --bin
nowbar --bin nowbar-settings`. Sürüm derlemesi kadar hızlı çalışan bir exe üretir ama programın
tamamını birlikte optimize etmez (LTO yoktur); bu yüzden önbellek doluyken derleme birkaç saniye
sürer. Ayarlar penceresinin arayüzü (`ui\`, Slint) bu profilde optimizasyonsuz derlenir: bir
`.slint` değişikliği 4–5 dakika yerine 40 saniyede derlenir.

Çıktılar `target\quick\` klasörüne yazılır; projeyi kısa bir yola klonla (MSVC'nin yol sınırı
260 karakterdir). İki exe (`nowbar.exe` ve `nowbar-settings.exe`) **aynı klasörde** olmalıdır: Now
Bar ayarlar penceresini kendi yanında arar. İkisini `bin\` klasörüne kopyalayıp oradan çalıştır.
"Windows ile başlat" açıksa başlangıç kaydı kendiliğinden en son çalıştırılan kopyaya güncellenir.

**Yayın:** `tools\make-setup.ps1`, bir GitHub sürümünde yer alan dosyaları `dist\` klasörüne
üretir: `nowbar-setup.exe` (iki exe içinde), `nowbar.exe`, `nowbar-settings.exe` (güncelleyicinin
indirdikleri) ve `SHA256SUMS.txt`. C çalışma zamanı statik olarak bağlanır; bu sayede exe'ler
Visual C++ Redistributable kurulu olmayan Windows'larda da açılır. Sürüm derlemesi
(`target\dist\`, birkaç GB) saklanır: ilk derleme 13–14 dakika sürer, sonrakiler yalnızca değişen
kısımları derler (kod değişikliğinden sonra 5 dakika kadar). `-Clean` bu derlemeyi sonunda siler,
`-Jobs` aynı anda kaç derleme işi çalışacağını belirler.

```
powershell -ExecutionPolicy Bypass -File tools\make-setup.ps1 -Jobs 6
```

### Yazı tipleri

Pil ve kart, exe'nin yanındaki `fonts\` klasörüne bakar: `OneUISans*.ttf` varsa One UI Sans'ı
(telefonundan alabilirsin), yoksa `Inter*.ttf`'yi, o da yoksa Windows'un Segoe UI Variable'ını
kullanır. Ayarlar penceresi Inter'i kendi içinde barındırır (`ui\fonts`, SIL Open Font License).

## Dosyalar

- Ayarlar: `%APPDATA%\NowBar\settings.json`
- Günlük: `%LOCALAPPDATA%\NowBar\log.txt`. Pilin ne zaman ve neden gizlendiği de buraya yazılır
  (`pill: hidden (nothing active)`, `media: Brave left (paused 10 min)` gibi).
- Takım armaları: `%LOCALAPPDATA%\NowBar\crests\`
- Haftanın müziği: `%LOCALAPPDATA%\NowBar\music.json` (yalnızca son yedi gün)
- Raftan çıkan dosyalar (küçültülmüş resimler, zip'ler) asıllarının yanına yazılır; oraya
  yazılamazsa `%LOCALAPPDATA%\NowBar\Raf\` klasörüne.

## Güncelleme

Now Bar açıldıktan bir dakika sonra ve ardından günde bir kez GitHub'dan son sürümü sorar
(`api.github.com`; sorgu dışında hiçbir şey gönderilmez). Yeni sürüm varsa tepsi menüsünde
**Güncelle: x.y.z** ve Ayarlar → Hakkında'da bir satır belirir. Tıklayınca iki exe indirilir,
`SHA256SUMS.txt` ile doğrulanır, eskilerinin yerine konur ve Now Bar yeniden başlar; ayarlar ve
otomatik başlatma olduğu gibi kalır. Adımlardan biri bile başarısız olursa eski exe'ler yerinde
kalır. Now Bar'ın yazma izni olmayan bir klasöre (Program Files gibi) kurulduysa sürüm sayfası
açılır. Güncelleme denetimi Ayarlar → Hakkında → Güncellemeleri denetle ile kapatılabilir.

## Gizlilik ve ağ

Now Bar'ın bağlandığı yerler (hiçbiri API anahtarı istemez):

- **Şarkı sözleri:** çalan şarkının sanatçısı, adı ve süresi `lrclib.net`'e, orada
  bulunamazsa NetEase Cloud Music'e (`music.163.com`) ve Kugou'ya (`lyrics.kugou.com`)
  gönderilir (Ayarlar → Medya → Şarkı sözleri ile kapatılabilir).
- **Hava durumu:** seçilen şehrin koordinatları Open-Meteo'ya gönderilir (`api.open-meteo.com`,
  `geocoding-api.open-meteo.com`). **Konumumu kullan** seçiliyse konum Windows'un konum
  hizmetinden alınır (Windows izin ister); yerin adını bulmak için yaklaşık 1 km'ye yuvarlanmış
  koordinatlar OpenStreetMap'e (`nominatim.openstreetmap.org`) gönderilir.
- **Maçlar:** takip edilen takımlar için ESPN (`site.api.espn.com`) ve EuroLeague/EuroCup
  (`feeds.incrowdsports.com`, `live.euroleague.net`); armalar da bu kaynakların görsel
  adreslerinden indirilir.
- **Güncelleme:** GitHub (`api.github.com`), yukarıda anlatıldığı gibi.
- **Claude'a sor:** yazdığın soru bilgisayardaki Claude Code'a verilir (`claude -p`, araçsız:
  dosyalarına ve komut satırına dokunamaz); Claude Code onu kendi oturumunla Anthropic'e gönderir.
  Now Bar'ın kendisi bir yere bağlanmaz.

Bu bilgisayarda kalanlar: haftanın müziği burada sayılır; **Metni kopyala** Windows'un kendi metin
tanımasını kullanır; **Paylaş** yalnızca Windows'un paylaşım penceresini açar, dosyalar orada
seçtiğin yere gider.

**Uygulama bildirimleri** açıksa Now Bar yalnızca bu bilgisayardan erişilebilen `127.0.0.1:20034`
ve `[::1]` adreslerini dinler (port doluysa Windows'un verdiği başka bir portu kullanır ve bunu
`%LOCALAPPDATA%\NowBar\api.json` dosyasına yazar). Kapalıyken hiçbir port açılmaz. Web
sayfalarından, ağdan ve bu bilgisayardaki diğer Windows kullanıcılarından gelen istekler
reddedilir; ayrıntılar [DOCS.md](DOCS.md)'de.

İzlenen klasörler: ShareX'in ekran görüntüsü klasörü ve Windows'un `Resimler\Screenshots`
klasörü (yeni bir görüntü pilde kısa bir süre görünür ve rafa eklenir; Ayarlar → Sistem → Ekran
görüntüleri). Claude Code'un adımları ve izin istekleri görev çubuğunda görünür; ekran paylaşırken
bunu göz önünde bulundur.

## Bilinen sınırlar

- **Windows 11 ya da Windows 10 (1809 ve sonrası).** Windows 10'da kartın arkası bulanıklaşmaz
  (düz zemin kullanılır) ve simgeler Segoe MDL2 Assets'ten gelir. Windows 10 desteği 0.2.1'de
  geldi; görev çubuğundaki yerleşimi henüz yalnızca simülasyonla denendi.
- **Galaxy Buds:** Buds2'de denendi; Buds2 Pro, Buds FE, Buds Core, Buds3, Buds3 FE ve Buds4 aynı
  protokolü kullanır, Buds Pro da tanınır. İlk Galaxy Buds, Buds+ ve Buds Live desteklenmez.
  GalaxyBudsClient ya da Samsung'un Galaxy Buds uygulaması bağlantıyı kullanırken Now Bar
  kulaklığı okuyamaz.
- **Duraklatılan medya 10 dakika sonra pilden kalkar** (Ayarlar → Medya → Duraklayınca kal).
  Başka bir etkinlik yoksa pil de o an gizlenir; müzik yeniden çalınca geri gelir.
- Ayarlar penceresi canlı verileri göstermez: maçın anlık skoru, Buds'ın o anki şarjı ya da
  çalışan zamanlayıcı yerine o etkinliğin pilinin örnek bir görünümü yer alır.
- Yalnızca ana ekranın görev çubuğunda çalışır.
- Boştaki kaynak kullanımı (medya duraklatılmışken, 2026-10-21 ölçümü): 10 saniyede 5–10 ms
  işlemci süresi, 39–66 MB çalışma kümesi (25 MB özel bellek). Hedef 10 saniyede 5 ms ve 50 MB'tı.
- Explorer yeniden başlarsa pil görev çubuğuna kendiliğinden geri döner; açık bir kart varsa
  kapanır.

## Geliştirme

```
cargo t
cargo lint
```

Testler ve clippy (`.cargo\config.toml`'daki kısaltmalar) quick profilinde çalışır; düz
`cargo test` ise bağımlılıkların ikinci bir tam kopyasını `debug\` altına derler (onlarca GB).

- Derlemeyi hızlandıranlar: Rust'ın kendi bağlayıcısı (`rust-lld`), bağımlılıklarda hata ayıklama
  bilgisinin kapalı olması ve ayarlar penceresinin Slint arayüzünün ayrı bir crate'te (`ui\`)
  bulunması: arayüz yalnızca bir `.slint` dosyası değişince yeniden derlenir.
- `NOWBAR_SETTINGS_SHOTS=<klasör> nowbar-settings.exe`: ayarlar penceresini ekran dışında açar,
  her sayfanın gerçek çiziciyle ekran görüntüsünü alır ve ayarları kaydetmeden kapanır.
- `pillsheet`, `cardsheet`, `matchsheet`, `settingsheet`: pencere açmadan örnek görüntüler
  üretir. `readmesheet` ile `tools\readme-images.py` bu sayfadaki görselleri yeniden çizer
  (`assets\readme\`).
- `tools\*.ps1`: titreme, boştaki kullanım, Explorer'ın yeniden başlatılması ve diğer ölçümler.
- `nowbar.exe --demo`: bir dakikalık örnek bir zamanlayıcıyla başlar; `--match-replay <klasör>`
  kaydedilmiş skor tablolarıyla bir maçı yeniden oynatır.
- `assets\make_icon.py`: simgeyi (tepsideki beyaz hap) yeniden çizer.
- **Sürüm yayınlamak:** `Cargo.toml`'daki `version`'ı artır, `tools\make-setup.ps1`'i çalıştır,
  sonra `dist\` içindekileri yayınla:
  `gh release create vX.Y.Z dist\nowbar-setup.exe dist\nowbar.exe dist\nowbar-settings.exe dist\SHA256SUMS.txt`.
  `nowbar.exe --update-from <klasör>` güncellemeyi GitHub olmadan, bir klasördeki exe'lerle dener.

## Lisans

Now Bar, [GNU Genel Kamu Lisansı sürüm 3](LICENSE) (GPL-3.0) ile lisanslıdır. Kodu
kullanabilir, değiştirebilir ve dağıtabilirsin; değiştirip dağıttığın sürümün kaynak kodunu da
aynı lisansla açman gerekir. Telif hakkı © 2026 Kaan.

Bu lisansın dışında kalanlar:

- Galaxy Buds'ın gürültü kontrolü simgeleri (`src/activities/buds.rs`, `NoiseControl::icon`)
  Samsung'un SystemUI'ından çizildi ve Samsung'a aittir; README görsellerinde de görünürler.
- Claude'un işareti (`src/render/pill.rs`, `CLAUDE_ICON`) Anthropic'e aittir; o da görsellerde
  görünür.
- Ayarlar penceresinin yazı tipi Inter, kendi lisansıyla gelir (SIL Open Font License,
  `ui\fonts\Inter-OFL.txt`).

---

<sub>Samsung ile bir ilgisi yoktur; One UI'dan esinlenmiş bağımsız bir projedir. Galaxy Buds'ın
gürültü kontrolü simgeleri Samsung'a aittir.</sub>
