# Piano di progetto: devstribution

## Problema
Per semplificare e velocizzare processi aziendali, spesso la soluzione più rapida è uno script Python. Il problema è distribuirlo ad aziende e clienti:
- `.exe` / `.bat` non firmati vengono bloccati da Microsoft (SmartScreen, antivirus).
- L'utente finale non è in grado di installare Python, dipendenze, ecc.

**Obiettivo:** una piattaforma che semplifichi la ricezione e l'utilizzo di piccoli applicativi, senza `.exe`/`.bat` da distribuire uno per uno.

## Requisiti emersi
| Tema | Decisione |
|---|---|
| Utenti finali | Chiunque (colleghi e clienti esterni, non tecnici) |
| Casi d'uso tipici | Interrogare file Excel, SAP, API |
| Dati | Devono restare **locali** sul PC dell'utente |
| Eccezione | Pacchetti diagnostici per chi ha sviluppato la app, solo su consenso dell'utente |
| Modello | Gratuito e open source, inizialmente su GitHub |
| Sostegno | Donazioni (PayPal / Patreon / Ko-fi / GitHub Sponsors) |

## Opzioni valutate
| Approccio | Esito |
|---|---|
| A. Server-side con UI generata | Scartato: i dati lascerebbero il PC dell'utente, costi di infrastruttura |
| B. Nel browser (Pyodide/WASM) | Insufficiente da solo: SAP (COM/RFC) e molte API (CORS) non raggiungibili. Utile solo per tool basati su Excel, eventuale modalità extra |
| C. Launcher locale firmato | **Scelto**: accesso a SAP, filesystem, rete interna; dati locali |

Alternative esistenti da studiare: Windmill, Streamlit, Gradio, NiceGUI, marimo / stlite / PyScript, Retool, Appsmith, **PyApp** (exe che al primo avvio scarica Python con `uv`), **Briefcase**.

**Differenziazione**: nessuna di queste unisce (1) un solo launcher firmato per molte app, (2) form generato dal manifest senza scrivere UI, (3) modello di fiducia per autori gestibile dall'IT senza server.

## Rischi principali (da verificare per primi)
| Rischio | Impatto | Mitigazione |
|---|---|---|
| **AppLocker / WDAC** bloccano eseguibili in cartelle scrivibili dall'utente (`%LOCALAPPDATA%`). I Python di `uv` (python-build-standalone) non hanno firma Authenticode; anche il `python.exe` dei venv è una copia dentro la cartella del venv | Le app non partono proprio nei PC aziendali, il target principale | **Fase 0**: test su un PC aziendale bloccato reale. Piano B: modalità per-macchina (vedi sotto) |
| **SignPath Foundation** chiede progetti open source già avviati | Launcher non firmato nei primi mesi → SmartScreen | Piano B: certificato OV a pagamento, oppure distribuzione via winget/Intune accettando l'avviso per i primi utenti |
| **SAP**: `pyrfc` archiviato da SAP, SDK NW RFC proprietario e non ridistribuibile; SAP GUI Scripting spesso disattivato dagli admin SAP | Uno dei casi d'uso chiave può non funzionare | Dichiarare i prerequisiti nel manifest (`requires`), il launcher li controlla e spiega cosa manca. Documentare cosa è supportato davvero |

### Modalità di installazione di Python
- **Per-utente** (default): `uv` gestisce Python e venv in `%LOCALAPPDATA%`. Funziona dove non c'è application control.
- **Per-macchina** (installer da admin / Intune): l'installer deposita uno o più Python in `Program Files`. Le app girano con quell'interprete, senza venv: dipendenze installate con `uv pip install --target` in una cartella per app, caricata via `sys.path` dal runner. Da verificare nella Fase 0 se le regole DLL di WDAC bloccano i `.pyd` delle dipendenze.

## Architettura scelta
### 1. Un solo programma firmato: launcher Tauri
- Installato una volta (installer MSI/NSIS generato da Tauri, winget, Intune), niente `.bat`.
- Firma: SignPath Foundation (gratuita per open source) o certificato OV come piano B.
- Niente PyInstaller (falsi positivi antivirus): il launcher è un eseguibile nativo.
- UI in WebView2, comunicazione con il backend via **IPC Tauri**: nessun server HTTP locale, quindi niente superficie CSRF / DNS rebinding.
- `uv` incluso come **sidecar**, firmato e installato insieme al launcher.

**Divisione del codice**
| Parte | Linguaggio | Responsabilità |
|---|---|---|
| Core | Rust | Verifica firme (`ed25519-dalek`), policy da HKLM (`winreg`), estrazione sicura, invocazione `uv`, sottoprocessi (Job Object), log, segreti (`keyring`) |
| UI | TypeScript | Catalogo, schermata permessi e conferma, form generato dal manifest, avanzamento ed esiti |
| Runner | Python (minimo) | Legge JSON degli input, chiama `run()`, scrive JSON dei risultati |

Regola: **nessuna logica del launcher in Python**. Python esiste solo dentro il runner, nell'ambiente dell'app.

### 2. Le app sono dati, non eseguibili
Ogni app è una cartella (repo GitHub o zip) con un `pyproject.toml` standard; il manifest sta in `[tool.devstribution]`. Il launcher legge il manifest, prepara l'ambiente dal lockfile e **genera il form**. Chi sviluppa non scrive UI.

### 3. Distribuzione
- Installazione con un link: `devstribution://install/<utente-github>/<app>`. **Porta sempre alla schermata di conferma**, anche in modalità gestita: qualunque pagina web può aprire il link, quindi non deve mai installare in silenzio.
- Catalogo = un repo GitHub con la lista delle app. Nessuna infrastruttura da mantenere.
- Aggiornamenti dalle release GitHub, versioni fissate con hash.

### 4. Privacy
- Tutto gira in locale, la piattaforma non riceve dati.
- **Diagnostica**: in caso di errore il launcher crea uno zip (log, versioni), applica una **redazione automatica** (percorsi utente, nome utente, variabili d'ambiente, valori dei segreti), **mostra all'utente il contenuto**, e lui decide se inviarlo (issue GitHub precompilata o file). Mai invio automatico.

### 5. Sicurezza (punto critico)
Eseguire script di terzi è rischioso:
- Permessi dichiarati nel manifest (rete, cartelle, SAP). **Sono informativi**: la UI lo dice esplicitamente ("l'autore dichiara che..."), per non dare un falso senso di sicurezza.
- Conferma dell'utente alla prima esecuzione di ogni app.
- Badge "verificata" per le app riviste nel catalogo.
- Hash/versioni fissate per evitare aggiornamenti malevoli silenziosi.
- Ogni esecuzione in un **Job Object** Windows: kill dell'intero albero di processi su timeout/Annulla, limiti di memoria opzionali.

## Modello di fiducia e policy
Principio: **robusto per l'IT, senza attrito per tutti gli altri**. Un solo launcher; il comportamento dipende dalla policy.

**Modalità**
- **Libera** (nessuna policy): avvisi forti e conferma alla prima esecuzione di ogni app, a rischio dell'utente.
- **Gestita** (policy in HKLM / GPO / Intune): esegue solo app firmate da autori approvati dall'IT.

**Ruoli** (solo due, nessun server da amministrare)
- **Publisher**: genera una chiave (un comando), firma e pubblica app.
- **Approver**: di solito l'IT; decide di quali autori fidarsi.

**Decisioni**
| Tema | Decisione |
|---|---|
| Chiavi | Chiavi di fiducia (Ed25519), non chiavi di licenza: nessun server, coerente con open source e dati locali |
| Formato firma | Ed25519 con formato compatibile **minisign** (stesso formato dell'updater di Tauri). Niente Sigstore: richiede rete e identità OIDC, in contrasto con "nessun server / offline" |
| Firma | Livelli opzionali: autore (sempre) → controfirma catalogo → controfirma IT. La policy dice quali richiedere |
| Policy | Un file TOML con campi tutti facoltativi (autori ammessi, versione minima launcher, mirror dipendenze, diagnostica on/off, rete). 2-3 modelli pronti: permissivo, standard, restrittivo |
| Autori esterni | File `.trust` pubblicato dall'autore (chiave pubblica, nome, dominio/repo verificabile). L'IT lo importa con un comando e approva tutte le app e versioni di quell'autore |
| Blocco | Il messaggio di blocco genera un `.trust` pronto da inoltrare all'IT ("chiedi al tuo IT di fidarsi di X") |
| Revoca | Lista firmata con cache e scadenza lunga (~30 giorni); può stare anche su share di rete. Rotazione: la chiave successiva è dichiarata e firmata dalla precedente |
| Enforcement | Policy in HKLM / Intune; non ci si difende da un admin locale (dichiarato nel modello di minaccia) |
| Audit | Log locale strutturato / Windows Event Log, raccoglibile dall'IT. La piattaforma non invia nulla |
| Aggiornamenti app | Default: nuova versione dello stesso autore fidato si aggiorna se firma e lockfile sono validi. Opzioni in policy: ring di attesa, approvazione manuale, pin |
| Aggiornamenti launcher | Plugin updater di Tauri (aggiornamenti firmati), disattivabile dall'IT (distribuzione via Intune/winget) |
| Dipendenze | La firma copre manifest + lockfile con hash; fuori lockfile = rifiuto. Mirror interno opzionale da policy |
| Supporto | Doc orientata all'IT, modalità dry-run della policy, nessuna funzione che richieda un server |
| Modello di minaccia | Una pagina: minaccia → chi la blocca → cosa NON copre. Coperti: autore malevolo o compromesso, link `devstribution://` da siti malevoli, pacchetti manomessi. Non coperti: admin locale, malware preesistente, bug nel codice di app firmate |

**Ordine di implementazione**: (1) firma autori + modalità libera/gestita + policy minimale, (2) `.trust` e import, (3) revoca, rotazione, log, (4) opzioni avanzate (ring, mirror, pin).

## Standard del pacchetto app (v1)
Obiettivo: ricevere un'app come **un file di dati**, verificarla e lanciarla in modo uniforme. Estensione: `.dsapp`.

### Formato del pacchetto
Uno zip (non un eseguibile, quindi non soggetto a SmartScreen) con questa struttura:
```
mia-app.dsapp
  pyproject.toml  # metadati Python standard + manifest in [tool.devstribution]
  uv.lock         # dipendenze con hash (obbligatorio)
  main.py         # codice (più moduli/asset a piacere)
  FILES.sha256    # elenco hash di tutti i file (formato canonico, vedi sotto)
  FILES.sha256.minisig  # firma Ed25519 dell'autore su FILES.sha256
```
Opzionale: cartella `wheels/` per l'uso offline (installazione senza internet).

**Formato canonico di `FILES.sha256`**: UTF-8, fine riga LF, una riga per file `<sha256 hex>  <percorso>`, percorsi con `/`, ordinati per byte. Tutti i file dello zip devono comparire, esclusi solo `FILES.sha256` e le sue firme; file in più o mancanti = rifiuto.

**Regole di estrazione** (rifiuto del pacchetto se violate): niente percorsi assoluti o con `..`, niente symlink, niente nomi duplicati (anche solo per maiuscole/minuscole), limite di dimensione totale e di numero di file.

### Manifest in `pyproject.toml`
```toml
[project]
name = "interroga-ordini"
version = "1.2.0"
requires-python = ">=3.11"
dependencies = ["openpyxl>=3.1"]

[tool.devstribution]
schema_version = 1
id = "autore/interroga-ordini"
title = "Interroga ordini"
author = { id = "autore", key = "<fingerprint>" }
entry = "main:run"              # modulo:funzione
platforms = ["windows"]
requires = []                    # prerequisiti esterni: "sap-gui-scripting", "sap-nwrfc-sdk", "excel"

[[tool.devstribution.inputs]]    # genera il form
name = "file_excel"
type = "file"                    # file | folder | text | number | bool | choice | date | secret
label = "File Excel degli ordini"
accept = [".xlsx"]

[[tool.devstribution.outputs]]
name = "report"
type = "file"                    # file | table | text

[tool.devstribution.permissions] # dichiarativi e informativi, mostrati all'utente
network = ["api.azienda.it"]
folders = ["user-selected"]
sap = false
secrets = ["sap_user"]           # nomi richiesti, valori nel keyring
```
Le dipendenze sono dichiarate una sola volta, in `[project]`, come in qualsiasi progetto Python.

### Contratto di esecuzione
```python
def run(file_excel: Path, ctx) -> dict:
    ctx.progress(0.3, "Leggo il file")
    ...
    return {"report": out_path}
```
- Il manifest è la **fonte unica** degli input; `devstribution dev` e `devstribution pack` verificano che la firma di `run()` corrisponda (stessi nomi e tipi compatibili) e rifiutano il pacchetto altrimenti.
- `ctx` espone `progress()`, `log()`, `output_dir`, `secrets.get(nome)`.
- Il ritorno è un dict con le chiavi degli `outputs`; i file vanno scritti in `ctx.output_dir`.
- Errori: un'eccezione = esito fallito, con traceback nel log locale e proposta del pacchetto diagnostico.

### Esecuzione
1. Il core Rust lancia un **sottoprocesso** `python -m devstribution_runner` nell'ambiente dell'app, dentro un Job Object.
2. Input e segreti passano come JSON su **stdin** (i segreti mai su file o variabili d'ambiente); i risultati tornano come JSON; stdout/stderr vanno al log. Il runner è una piccola libreria pinnata dal launcher.
3. Cartella di lavoro temporanea per esecuzione, con timeout e pulsante "Annulla".
4. Ambiente creato con `uv sync --frozen` dal lockfile (o `uv pip install --target` in modalità per-macchina); Python gestito da `uv`; cache condivisa tra app.

### Flusso di installazione
`ricezione → estrazione sicura in area temporanea → verifica hash, firma e policy → controllo prerequisiti (requires) → mostra manifest e permessi → conferma (o approvazione da policy) → spostamento in cartella indirizzata per hash → creazione ambiente → pronta`

Più versioni della stessa app possono coesistere; la vecchia si rimuove dopo l'aggiornamento riuscito.

### Canali di ricezione (tutti equivalenti, stesso flusso)
| Canale | Uso |
|---|---|
| Doppio clic su `.dsapp` | Associazione file registrata dall'installer Tauri; allegato email, share di rete |
| `devstribution://install/<autore>/<app>` | Link da catalogo o pagina web (plugin deep-link di Tauri); scarica dalla release GitHub. Sempre con conferma |
| Da catalogo nel launcher | Elenco da repo GitHub |
| `devstribution dev <cartella>` | Per lo sviluppatore: esegue una cartella non firmata solo in modalità libera |

### Cartelle locali
```
%LOCALAPPDATA%\devstribution\
  apps\<id>\<versione-hash>\   # codice estratto
  envs\<id>\<versione-hash>\   # venv (o cartella dipendenze in modalità per-macchina)
  runs\<timestamp>\            # output e log per esecuzione
  logs\                        # log del launcher
```

### Da decidere ancora per il formato
- Proposta per la v1: input `file, folder, text, number, bool, choice, secret`; `date` e output `table` nella v1.1; `ctx.progress` già nell'MVP (costa poco ed è molto visibile per l'utente).

## Setup del progetto
Disponibilità del nome verificata il 2026-10-21 (tutto libero; per `.io` ricontrollare sul registrar).

**Subito**
- [ ] 2FA attiva sull'account GitHub personale.
- [ ] Organizzazione GitHub `devstribution` (piano Free): 2FA obbligatoria per i membri; Dependabot alerts, secret scanning, push protection, private vulnerability reporting attivi.
- [ ] Repo unico `devstribution/devstribution` (monorepo: core Rust, UI, runner Python). Il repo `catalog` arriva più avanti.
- [ ] Protezione del ramo `main`: no force-push, no cancellazione.
- [ ] Rinominare la cartella locale da `devstributor` a `devstribution` (a sessione chiusa).

**Questa settimana**
- [ ] Dominio `devstribution.dev` (eventualmente anche `.com`) via Cloudflare Registrar o Porkbun.
- [ ] Dominio verificato sull'organizzazione GitHub (serve per la pagina dell'organizzazione, per i `.trust` e per GitHub Pages).

**File iniziali del repo**
| File | Perché |
|---|---|
| `LICENSE-MIT`, `LICENSE-APACHE` | Doppia licenza MIT OR Apache-2.0 |
| `README.md` | Problema in 3 righe, stato "pre-alpha", link a `docs/plan.md` |
| `SECURITY.md` | Segnalazione privata delle vulnerabilità; richiesto in pratica anche da SignPath |
| `.github/FUNDING.yml` | Pulsante Sponsor (Ko-fi / Patreon / PayPal); GitHub Sponsors per l'org più avanti (richiede Stripe) |
| `.gitignore` | Rust, Node, Python |
| `docs/plan.md` | Questo piano |

**Quando c'è codice vero**
- [ ] crates.io: login con GitHub, prima pubblicazione con una versione funzionante (niente pacchetti vuoti per occupare il nome).
- [ ] PyPI: account con 2FA, Trusted Publisher da GitHub Actions per `devstribution-runner`.

**Più avanti**
- [ ] Domanda a SignPath Foundation dopo la prima release pubblica con documentazione minima.
- [ ] Pubblicazione su winget con installer firmato.

## Roadmap MVP
**Fase 0: fattibilità (prima di tutto il resto)**
Su un PC aziendale bloccato reale (AppLocker/WDAC attivi), verificare con uno script:
1. `uv` dal sidecar firmato in `Program Files` parte?
2. Python di `uv` in `%LOCALAPPDATA%` parte? E il `python.exe` di un venv?
3. Python in `Program Files` + dipendenze in `%LOCALAPPDATA%` caricate via `sys.path` (inclusi `.pyd`, es. `pandas`)?
4. Accesso a Excel/SAP GUI Scripting dal processo figlio.

L'esito decide la modalità di installazione di default.

**Fase 1: prototipo da mettere in mano a 2-3 colleghi**
1. Launcher Tauri minimo: UI con form generato dal manifest, IPC verso il core Rust.
2. Esecuzione in sottoprocesso (Job Object) con ambiente da lockfile, runner Python, `ctx.progress`.
3. App di esempio: carica un Excel, lo interroga, scarica il risultato.
4. Installazione da file `.dsapp` (associazione file) con estrazione sicura.
5. Comando `devstribution dev` / `pack` per lo sviluppatore.

**Fase 2: fiducia**
6. Firma e verifica Ed25519 (formato minisign) dell'autore, `FILES.sha256`.
7. Modalità libera/gestita con policy minimale da HKLM.
8. Installazione da URL GitHub e schema `devstribution://`.

## Fasi successive
- Firma del launcher (SignPath o OV), pubblicazione su winget, pacchetto MSI per Intune.
- `.trust`, revoca, rotazione chiavi, log nell'Event Log.
- Catalogo pubblico e badge di verifica.
- Pacchetto diagnostico con redazione.
- Modalità opzionale nel browser (Pyodide) per app solo-Excel.
- Link di donazione nel launcher e nel README.
- Supporto macOS/Linux (Tauri lo rende possibile; app SAP restano solo Windows).

## Questioni aperte: raccomandazioni
| Questione | Raccomandazione |
|---|---|
| Nome | **Deciso: devstribution.** Liberi al 2026-10-21: GitHub, crates.io, PyPI (`devstribution`, `devstribution-runner`), domini `.com/.dev/.org/.io`. Passi di registrazione in "Setup del progetto" |
| Piattaforme | **Windows prima**, ma non chiudere la porta: campo `platforms` nel manifest e core Rust senza dipendenze Windows fuori da moduli dedicati |
| Licenza | **MIT OR Apache-2.0** (doppia, come Rust, Tauri e uv): permissiva, gradita alle aziende, Apache porta la clausola sui brevetti. Il runner con la stessa licenza, così le app possono avere qualunque licenza |
| Segreti | Windows Credential Manager via crate `keyring`, una voce per app e nome segreto. L'app riceve solo i segreti dichiarati nel manifest, via stdin; il launcher li chiede all'utente al primo uso e li redige dai log |
| Badge "verificata" | Controfirma del catalogo per **singola versione**, dopo una checklist: sorgente pubblico, pacchetto ricostruibile dal tag (build in GitHub Actions), niente codice offuscato o scaricato a runtime, permessi coerenti con il codice. Se manca la revisione, l'app resta installabile ma senza badge |
| Strumento di firma | Implementazione Ed25519 nel core Rust, **formato compatibile minisign** (interoperabile, verificabile con strumenti esistenti, stesso formato dell'updater di Tauri). Niente Sigstore |
| Estensione e schema URL | **Decisi**: estensione `.dsapp`, schema `devstribution://`. Verificare che `.dsapp` non sia già associata a software diffuso |
| Catalogo e revoca | Repo nell'org GitHub del progetto, servito via GitHub Pages. Chiave del catalogo **offline**, non nei segreti di CI; firma della lista di revoca con la stessa chiave. Mirror su share di rete previsto dalla policy |
| Sandbox | v1: permessi informativi + Job Object (timeout, kill albero, limiti memoria). Più avanti valutare AppContainer o blocco rete via Windows Firewall per app, solo se il costo è ragionevole; dichiararlo nel modello di minaccia |
| Python per-macchina | Decidere dopo la Fase 0 quale modalità è il default e quali versioni di Python l'installer per-macchina include |
