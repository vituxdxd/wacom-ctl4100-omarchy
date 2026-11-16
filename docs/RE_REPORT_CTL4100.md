# Relatório de engenharia reversa — Wacom Intuos S (CTL-4100)

Versão analisada: **Wacom Tablet 6.4.15-2** (macOS), instalador
`WacomTablet_6.4.15-2.dmg`.

Este documento separa três tipos de afirmação:

- **[OBS]** observado diretamente no instalador, no descritor HID de hardware
  real ou no código-fonte do kernel Linux;
- **[INF]** inferência a partir de símbolos/descompilação, sem execução em
  hardware nesta máquina;
- **[IMPL]** decisão da nossa reimplementação em Rust, quando o binário oficial
  não expõe o valor exato.

A mesa não estava conectada durante este trabalho. Nenhuma afirmação abaixo
depende de captura ao vivo feita aqui.

---

## 1. Cadeia de extração

### 1.1 Hashes e tamanhos

| Artefato | Tamanho | SHA-256 |
| --- | ---: | --- |
| `WacomTablet_6.4.15-2.dmg` | 197.196.999 B | `84af01a189e7416b01c2f37c51e3f47ae16de60b6211bfc6cdd190d2567c90c0` |
| `Install Wacom Tablet.pkg` | 194.261.184 B | `d507ec51699e01641800cd91dccbc0d0bb1157a4cc45b7d20f487f3f4000e95c` |
| `WacomTabletDriver` (slice x86_64) | 24.909.936 B | `49eae096793585f7b4dafbab2976e40075b4b2bf2bfcaafc490944c21c04e3f3` |
| Descritor HID `0003:056A:0374.0005.hid.bin` | 759 B | `a97d2c3ef0d3e50f9ad84e0cae3e2a0fe51db5d334e8be0c815735c15008ad55` |

### 1.2 Camadas

```
.dmg (UDIF, Apple DMG v4)
  └─ 7z x                → "Install Wacom Tablet.pkg"  (xar)
       └─ xar -xf        → content.pkg/{Bom,PackageInfo,Payload,Scripts}
            └─ Payload   → gzip (1f 8b 08 00)
                 └─ gunzip | cpio -idm
                      → 6.479 arquivos regulares, 391 MB
```

- **[OBS]** `7z l` lista o DMG com um único `.pkg` e 5 arquivos no total.
- **[OBS]** `xar -tf` lista 61 entradas; `PackageInfo` declara
  `payload numberOfFiles="6790" installKBytes="388471"`.
- **[OBS]** o `Payload` começa com `1f 8b 08 00` (gzip), não `pbzx`; o
  `pbzx` não foi necessário.
- **[OBS]** duas entradas de `/Library/LaunchDaemons` não puderam ser escritas
  pelo usuário sem root; o restante foi extraído.
- **[OBS]** `Library/Extensions/` está **vazio**: a versão 6.4.15 não instala
  `.kext`. O driver é um processo de usuário (`WacomTabletDriver.app`) falando
  IOHID, mais helpers privilegiados (`Wacom_IOManager`,
  `com.wacom.UpdateHelper`, `com.wacom.DataStoreMgr`).

### 1.3 Bundles relevantes

| Caminho | ID | Papel |
| --- | --- | --- |
| `.Tablet/WacomTabletDriver.app` | `com.wacom.wacomtablet` | driver HID + lógica de todas as mesas (49 MB) |
| `.Tablet/TabletDriver.app` | `com.wacom.TabletDriver` | lançador/agent |
| `Wacom Center.app` + `WacomCenter.prefpane` | `com.wacom.WacomCenter` | central de configuração nova |
| `WacomTablet.prefpane` | `com.wacom.ProfessionalControlPanel` | "Wacom Tablet Properties" legado |
| `.Tablet/FirmwareUpdater.app` | `com.wacom.FirmwareUpdater` | atualização de firmware |
| `.Tablet/WacomADCV.app` | `com.wacom.WacomADCV` | diagnóstico/ADC |
| `.Tablet/WacomTouchDriver.app` | `com.wacom.WacomTouchDriver` | toque (não se aplica à CTL-4100) |
| `Wacom Display Settings.app` | `com.wacom.Wacom-Display-Settings` | mapeamento de tela |

### 1.4 Binário analisado

`WacomTabletDriver` é um fat binary `x86_64` + `arm64`. A slice x86_64 tem
**122.654 símbolos definidos** (não foi stripped), incluindo nomes completos de
classes e caminhos de origem:

```
/Users/wacom/Development/R_6.4.15/Wacom/Mac/Tools/buildbot/R_6.4.15/
  Wacom/Common/TabletDriver/CCTL4100GraphicsTablet.cpp
  Wacom/Common/TabletDriver/CCTL4100Pen.cpp
  Wacom/Common/TabletDriver/CCTL4100TabletControls.cpp
```

Ferramentas: `llvm-lipo -thin x86_64`, `llvm-nm -C`, `llvm-objdump`,
`strings`, Ghidra 12.2 headless (`DumpDecomp.java`, descompilação por nome de
símbolo). O projeto Ghidra reutilizado está em `/tmp/opencode/ghidra_wacom`.

---

## 2. HID / USB

### 2.1 Identificação

| Item | Valor | Evidência |
| --- | --- | --- |
| VID | `0x056A` (Wacom) | `lsusb`/descritor |
| PID CTL-4100 | `0x0374` | descritor, libwacom, `dmesg` |
| PID CTL-4100WL USB | `0x0376`, `0x03C5` | libwacom |
| PID CTL-4100WL BT | `0x0377`, `0x03C6` | libwacom |
| Produto | `Intuos S` | string USB |
| Classe | HID v1.10, full-speed | `dmesg` de hardware real |
| Interface | 1 interface HID, 1 endpoint IN | descritor |

**[OBS]** O PID `0x0375` citado no pedido não aparece na base libwacom nem no
descritor. Os IDs reais da linha CTL-4100WL são `0x0376/0x0377` (primeira
geração) e `0x03C5/0x03C6` (refresh). O daemon abre os três PIDs USB
(`0374/0376/03C5`); os PIDs Bluetooth são reconhecidos no relatório, mas não
implementados.

### 2.2 Descritor de relatório

Descritor real da CTL-4100 (`Wacom Intuos S (3rd-gen)`, dump
`0003:056A:0374.0005.hid.txt`, 759 bytes), coleção vendor page `0xFF0D`,
`Usage (0x01)`, `Report ID 16`:

| Report ID | Tipo | Payload | Uso |
| ---: | --- | ---: | --- |
| `0x10` (16) | Input | 26 B | caneta (posição, pressão, botões, tilt) |
| `0x11` (17) | Input | 8 B | 4 ExpressKeys + padding |
| `0x13` (19) | Input | 8 B | bateria (modelos wireless) |
| `0xAC` (172) | Input | 191 B | stream vendor "raw" usado pelo driver macOS |
| `0x02` (2) | Feature | 1 B | modo do dispositivo (HID ↔ Wacom) |
| `0x0C` (12) | Feature | 8 B | offsets/área (4 × u16) |
| `0x0D` (13) | Feature | 1 B | scan da caneta |
| `0x14` (20) | Feature | 13 B | serial number |
| `0x31` (49) | Feature | 3 B | brilho de LEDs (o driver envia 5 argumentos) |
| `0xCD` (205) | Feature | 2 B | reset de hardware (o driver envia 3 bytes) |

O descritor define os Feature Reports 2, 3, 4, 7, 12, 13, 20, 49, 204, 205 e
vários blobs vendor (21, 51, 68, 69, 96–111, 172, 208–228). Nós usamos apenas
os que têm semântica confirmada.

**[OBS]** O binário também envia os report IDs `0x0E` (scan do toque), `0x32`
(LED do anel), `0x34` (mass storage) e `0x82` (status Bluetooth). Esses IDs não
aparecem no descritor USB desta revisão: pertencem a modelos com toque/BT. A
CTL-4100 com cabo não os utiliza, e nosso driver não os envia.

### 2.3 Input Report `0x10` — caneta

Payload de 26 bytes (o buffer do hidraw tem 27, incluindo o Report ID):

| Offset | Bits | Campo | Faixa / semântica |
| ---: | ---: | --- | --- |
| 0 | 1 | `TipSwitch` (usage `0x42`) | 1 = ponta tocando |
| 0 | 1 | `BarrelSwitch` (`0x44`) | 1 = botão lateral inferior |
| 0 | 1 | `BarrelSwitch2` (`0x5A`) | 1 = botão lateral superior |
| 0 | 2 | constante | — |
| 0 | 1 | `InRange` (`0x32`) | 1 = caneta na área |
| 0 | 1 | vendor `0x36` | interpretado como eraser (não usado na LP-1100K) |
| 0 | 1 | constante | — |
| 1–3 | 24 | `X` LE | 0…15200 (100 unidades/mm) |
| 4–6 | 24 | `Y` LE | 0…9500 |
| 7–8 | 16 | `TipPressure` LE | 0…4095 (256 níveis por bit útil) |
| 9–14 | 48 | constante | 6 bytes |
| 15 | 8 | `Z` / distance | 0…63 |
| 16–19 | 32 | `TiltX` (i32) | não usado pela CTL-4100 (OAK) |
| 20–23 | 32 | `TiltY` (i32) | não usado pela CTL-4100 (OAK) |
| 24–25 | 16 | vendor `0x77` | não usado |

**[OBS]** O descritor fixa `Logical Maximum 15200` para X, `9500` para Y e
`4095` para pressão.
**[OBS]** `CGD16StylusTransducer::HandleTransducerData` (0x10023C642) lê a
pressão em `packet[8] | packet[9] << 8`, exatamente o offset 7–8 do payload,
e descarta tilt quando `IS_OAK(sensorID)` é verdadeiro.
**[OBS]** `CCTL4100GraphicsTablet::DeviceStart` (0x1000DFEBE) chama
`SetMaxPressureLevels(0x1000)`.
**[OBS]** `CCTL4100GraphicsTablet::Init` (0x1000DFC62) configura
`SetPacketSize(0x1B)` = 27 bytes no modo USB e `0x169` = 361 bytes no modo
Bluetooth.

### 2.4 Input Report `0x11` — ExpressKeys

| Offset | Bits | Campo |
| ---: | ---: | --- |
| 0 | 0–3 | ExpressKeys A, B, C, D (bit 0 = A) |
| 0 | 4–7 | constante |
| 1–7 | 56 | constante (7 bytes) |

**[OBS]** `CCTL4100TabletControls::ProcessCurrentTagList` (0x1003E80DE) faz
`packet[1] & 0xF`, publica em `ERawTabletButtons` e compara com o estado
anterior para gerar o HUD.

### 2.5 Input Report `0x13` — bateria (WL)

| Offset | Bits | Campo |
| ---: | ---: | --- |
| 0 | 0–6 | nível 0…100 |
| 0 | 7 | carregando |
| 1 | 8 | desconhecido |
| 2–7 | 48 | constante |

### 2.6 Feature reports e sequência de inicialização

O driver macOS envia feature reports com
`IOHIDDeviceSetReport(kIOHIDReportTypeFeature)`; o primeiro byte do vetor é o
Report ID. Evidências:

- **[OBS]** string `_IOHIDDeviceSetReport` e
  `CMacHIDCommInterface::SendUSBCommand:EFeatureSetReport` no binário.
- **[OBS]** `CMacHIDCommInterface::SendUSBCommand` (0x1000A3000) passa
  `vector.data()` direto para `MacHidDevice::SendFeature`, com loop de retry.
- **[OBS]** `MacHidDevice::SetToWacomMode` (0x1002E0416) monta
  `{0x02, 0x02}`; `MacHidDevice::SetToHIDMode` (0x1002E0364) monta
  `{0x02, 0x01}`.
- **[OBS]** `CUSBCommInterface::SetToHIDMode` (0x100397C9E) também envia
  `{0x02, 0x01}`.

Sequência adotada (e implementada em `wacomd`):

```text
1. abrir hidraw (056a:0374)
2. SET_FEATURE 02 02        # sair do modo mouse/HID, entrar no modo Wacom
3. esperar ~40 ms
4. SET_FEATURE 0d 00        # habilitar scan da caneta (0 = on, 1 = off)
5. ler input reports 0x10 / 0x11
```

Comandos de feature confirmados no binário:

| Bytes | Função | Símbolo / endereço |
| --- | --- | --- |
| `02 02` | modo Wacom | `MacHidDevice::SetToWacomMode` 0x1002E0416 |
| `02 01` | modo HID/mouse | `MacHidDevice::SetToHIDMode` 0x1002E0364 |
| `0D 00/01` | scan da caneta on/off | `GD16SetScanModePen` 0x100399302 |
| `0E 00/01` | scan do toque on/off | `GD16SetScanModeTouch` 0x10039939C |
| `CD A6 50` | reset de hardware | `GD16SendHardwareReset` 0x100399DDE |
| `31 ...` | brilho de LEDs (5 bytes) | `GD16SetLEDBrightness` 0x100399214 |
| `32 ...` | LED do anel (2 bytes) | `GD16SetRingModeLED` 0x10039916A |
| `34 ...` | mass storage (2 bytes) | `GD16SetMassStorage` 0x100399636 |
| `14 00…` | consulta de serial (14 bytes) | `GD16GetSerialNumber` 0x100399436 |
| `03 00` | relatório OEM (2 bytes) | `GD16GetOEMReport` 0x1003994B6 |
| `0C 00…` | offsets (9 bytes) | `GD16GetOffsetReport` 0x100399536 |
| `16 00…` | info Bluetooth (15 bytes) | `GD16GetBluetoothInformation` 0x1003995B6 |
| `82 …` | status Bluetooth (49/50 bytes) | `GD16BTGetStatus` / `GD16BTSetStatus` |

**[OBS]** O kernel Linux confirma o mesmo comando de modo:
`wacom_hid_set_device_mode()` (drivers/hid/wacom_sys.c) procura o campo de
feature com usage `HID_DG_INPUTMODE` (o Report ID 2 do descritor), escreve o
valor **2** e faz `HID_REQ_SET_REPORT`. Em seguida `wacom_set_device_mode()`
envia `rep_data[0] = mode_report (2)`, `rep_data[1] = mode_value (2)` e relê o
relatório. O descritor da CTL-4100 declara `Logical Minimum (2)` e
`Logical Maximum (2)` para esse campo — o valor 2 é o modo esperado.

**[INF]** `SetToHIDMode` envia 1, valor fora do min/max declarado; isso é
aceito pelo firmware, mas não é usado pelo nosso driver.

### 2.7 Taxa, resolução e área

| Grandeza | Valor | Evidência |
| --- | --- | --- |
| Resolução | 2540 lpi = 100 unidades/mm | descritor (`Unit Exponent 13`, cm) e libwacom |
| Largura lógica | 15200 | descritor |
| Altura lógica | 9500 | descritor |
| Área ativa | 152 × 95 mm | descritor |
| Pressão | 0…4095 (4096 níveis) | descritor + `DeviceStart` |
| Distância | 0…63 | descritor (`0x0132`) |
| Taxa | configurável por `ForcePacketTiming` | `CCTL4100GraphicsTablet::EffectivePacketTiming` |

**[INF]** A taxa nominal não é uma constante recuperada: o driver oficial lê
`//ForcePacketTiming` das configurações e converte pps → µs
(`PPS_TO_MICROSECONDS`). A nossa telemetria mede a taxa efetiva (pps) em vez de
impor uma.

### 2.8 Modo "raw" `0xAC`

**[OBS]** O driver macOS trata o relatório de 27 bytes como uma sequência de
sub-relatórios de 8 bytes no caminho `HidReport::TranslateShortEMR`
(0x10040CFD9) e em `CCTL4100GraphicsTablet::ProcessDataPacket`
(0x1000E02C2). Cada sub-relatório tem o bit 7 do primeiro byte como
continuação e é expandido para um pacote interno de 16 bytes começando em
`0x10`, com padding inserido.

**[IMPL]** Nós **não** usamos esse caminho. O descritor HID define campos
estruturados inequívocos para o Report `0x10`, e é assim que o driver `wacom`
do Linux lê a mesa. O caminho de 8 bytes é preservado no relatório como
observação, mas o parser implementado segue o descritor (X/Y 24 bits,
pressão 16 bits).

---

## 3. Algoritmos do driver oficial

### 3.1 Curva de pressão

**[OBS]** `CPressureCurve` herda de `CBezierCurve`:

- construtor `CPressureCurve::CPressureCurve(unsigned short)` (0x1001AABF6)
  chama `CBezierCurve::CBezierCurve(EBezierCurveType, int)` com tipo 2 e
  `0x20` (32 amostras);
- `CPressureCurve::EvaluateAtValue(double)` (0x1001AADD2):
  - acima do ponto final → retorna `GetMaxPressure()` (0x1000);
  - abaixo do ponto inicial → avalia a lookup table (clamp em 0);
  - no meio → `CLookupTableCurve::EvaluateAtValue` e `max(0, y)`;
- `CPressureCurve::EvaluateAll()` (0x1001AAE28) pré-calcula a tabela;
- `CPressureCurve::SetPointTable` aceita pares `SDoublePoint` e seleciona a
  sub-tabela usada pelo Bézier;
- `CCTL4100Pen` usa `CGD16StylusTransducer`, que publica a pressão crua
  (`PutRawPressureInTaglist`) e depois a converte.

**[IMPL]** Nossa implementação:

- Bézier cúbica de `(0,0)` a `(1,1)` com dois pontos de controle livres;
- `TipFeel` 0…100 % interpola entre soft / linear / firm:
  - soft: `P1=(0.05,0.55)`, `P2=(0.35,0.98)`
  - linear: `P1=(1/3,1/3)`, `P2=(2/3,2/3)`
  - firm: `P1=(0.62,0.10)`, `P2=(0.95,0.45)`
- inversão de `x(t)` por bisseção (24 iterações) e LUT de 33 pontos;
- os pontos exatos dos presets do driver oficial **não foram recuperados**
  (ficam na GUI/config, não em constantes do binário). A arquitetura
  Bézier + LUT, essa sim, é a observada.

### 3.2 Coordenadas, suavização e duplo clique

**[OBS]** `CCTL4100Pen::HandleTransducerData` (0x1000DC39A) chama
`CTransducer::CheckDoubleClickAssist` (0x10003731EE) quando o byte `0x39` do
objeto está em 1. `CButton::FixUpPressure` (0x100021202) existe no caminho de
botões.

**[OBS]** Strings e símbolos de suavização/rotação:
`PlatformDoubleClick`, `GetDoubleClickTime`,
`PressureFixedDepthFilter`, `PressureSafteyMargin`, `RotationHelper`,
`ProcessRotation`, `InterpolateRotation`, `PutRawRotationInTagList`.

**[IMPL]** O duplo clique é implementado como *snap*: se dois toques da ponta
ocorrem dentro de `double_click_distance` pixels e `double_click_time_ms`, a
segunda posição é substituída pela primeira antes do evento de pressão. É o
comportamento que o "Tip Double Click Distance" do Wacom Center produz. O valor
padrão nosso é 10 px / 400 ms; os valores exatos do driver oficial não foram
extraídos.

### 3.3 Mapeamento espacial

**[OBS]** Classes do driver:

- `CMappingInputTablet::GetInputArea` (0x10030A394), `GetSelectedArea`,
  `SetAspectType`, `SetMappingOptions`, `SetIsInsetTabletModeOn`;
- `CMappingOutputScreen::GetMonitorArea` (0x100310E58),
  `CMappingOutputScreen::GetMonitor` (0x10022744C);
- `CMappingGroup` / `CMappingSet` (grupos de mapeamento por tela);
- `SMatrix` (48 métodos) para a transformação.

**[IMPL]** Nossa transformação, em `wacom-proto::transform`:

1. normaliza a coordenada nativa dentro da área ativa:
   `u = (x − x0)/w`, `v = (y − y0)/h`, clamp 0…1;
2. aplica rotação no sentido horário:
   - 0°: `(u,v)`
   - 90°: `(1−v, u)`
   - 180°: `(1−u, 1−v)`
   - 270°: `(v, 1−u)`
3. se `Force Proportions`, ajusta um retângulo de mesma proporção da mesa
   (152:95) dentro do alvo, centralizado;
4. projeta no retângulo de destino (layout inteiro ou um monitor);
5. normaliza o pixel do layout para `0…32767` no eixo absoluto do uinput.

A rotação 90° troca a proporção para 95:152, como esperado. A precisão
("FinePoint") aplica `prev + (next − prev) × 0.5`.

---

## 4. Mapeamento funcional do software

### 4.1 Ações mapeáveis

**[OBS]** O binário contém o enum `ECommandType` com dezenas de ações. As que
interessam à CTL-4100:

| Grupo | Ações observadas |
| --- | --- |
| Teclado | `EKeystroke`, `EKeystrokes`, `EModifiers`, `EKeyboardGroup`, `EUndo`, `ERedo` |
| Mouse | `ELeftClick`, `ERightClick`, `EMiddleClick`, `ELeftDoubleClick`, `ERightDoubleClick`, `EMiddleDoubleClick`, `ELeftClickLock`, `EMiddleClickLock`, `ERightClickLock` |
| Scroll/Zoom | `EScroll`, `EScrollUp`, `EScrollDown`, `EZoom`, `EZoomIn`, `EZoomOut`, `EAutoScrollZoom`, `EPenButtonPanScroll` |
| Modos | `EFinePoint`, `EModeToggle`, `EPressureHold`, `ETiltToggle`, `ETouchToggle`, `EDialToggle` |
| Tela/Janela | `EDisplayToggle`, `EShowDesktop`, `EShowAllWindows`, `EShowLaunchpad`, `ESwitchApp`, `ESnapWindowLeft`, `ESnapWindowRight`, `ESwitchVDLeft`, `ESwitchVDRight`, `EShowNextFullScreenApp`, `EShowPreviousFullScreenApp` |
| Wacom | `ERadialMenu`, `EShowRadialMenuPanel`, `EShowOnScreenPanel`, `EShowGridPanel`, `EToggleWacomCenter`, `EWacomDisplaySettings`, `EToggleControlPanel` |
| Apps | `ERunApplication`, `ERunOneNote`, `ERunScreenSketch`, `ERunStickyNotes`, `EShowDictionary`, `EFileExplorer`, `EAppDefined` |
| Sistema | `EChangeSettings`, `EInternalOSD`, `EDisplayToggle`, `EScreenRotationLockToggle`, `ERunActionCenter`, `EStartMenu` |
| Especiais | `EDisabled`, `ESeparator`, `ESkip`, `EJournal`, `EMacro_Deprecated` |

Classes que executam ações: `CWacomFunction_Keystroke`,
`CWacomFunction_SimpleEvent`, `CWacomFunction_RunApp`,
`CWacomFunction_RadialMenu`, `CWacomFunction_PenMouseMode`,
`CWacomFunction_EventPacket`, `CWacomFunction_Disabled`.

**[IMPL]** Nosso subconjunto: tecla+modificadores, clique esquerdo/direito/
meio, duplo clique, scroll, pan/scroll, modo precisão, alternar monitor, menu
radial e comando shell. Ações de janela/OS não fazem sentido em Wayland via
uinput; podem ser expressas como atalhos do Hyprland via `Action::Key`.

### 4.2 Telas do Wacom Center / Tablet Properties

**[OBS]** Prefpane e recursos incluem `Devices.strings`,
`Help_<Modelo>.nib`, `Localizable.strings` (valores em base64), o
`BezelWindowContent.nib`, telas de calibração e o "Wacom Experience Program".
A estrutura de abas é a mesma documentada pelo produto:

- **Pen**: botões da caneta, Tip Feel (curva), pressão de teste, duplo clique;
- **ExpressKeys**: 4 teclas, ações, HUD na tela, menu radial;
- **Mapping**: tela/monitor, área da mesa, proporções, orientação;
- **Touch**: ausente na CTL-4100;
- **Devices / About**: firmware, diagnóstico, perfis;
- **Perfis por aplicativo**: o driver mantém `CMappingSet`/`CMappingGroup` por
  app e o painel oferece "Application Specific Settings".

**[OBS]** `CCTL4100TabletControls::CreateControls` (0x1003E79A0) e
`CCTL4100GraphicsTablet::CreateTabletMenuArea` (0x1000E020C) criam a área de
controles da mesa; `ProcessEventForType`/`GetNthControlForType` tratam os
quatro botões. O HUD de teclas é controlado por `GetShowHUD` e pelo elemento
`0x8D` da tag list.

### 4.3 Diagnóstico em tempo real

**[OBS]** O binário tem `CGD16GraphicsTablet::HexDump`, contadores de pacotes
(`SetPacketCount`, `SetPacketPosition`), `CTabletTimeStamp` e logs
`CTL4100GraphicsTablet: PacketSize = %d`, `%s mode`.

**[IMPL]** A GUI mostra X, Y, pressão curvada e crua, distância, tilt,
in-range, tip, barrel 1/2, eraser, bitmask das 4 teclas, pps e monitores.

---

## 5. Reimplementação em Rust

Workspace Cargo com quatro crates:

| Crate | Conteúdo |
| --- | --- |
| `wacom-proto` | parser HID zero-alocação, feature reports, Bézier + LUT, transformação afim, config, IPC |
| `wacomd` | hidapi → uinput, Hyprland IPC, servidor Unix socket, perfis, ações |
| `wacomctl` | clap + cliente IPC |
| `wacom-gui` | eframe/egui no Wayland |

Decisões:

1. **Report `0x10` estruturado**, não o stream `0xAC` (ver §2.8).
2. **uinput com eixos normalizados no layout** (`0…32767`) em vez de unidades
   nativas: permite mapear para um monitor/região sem depender da configuração
   `input:tablet:output` do Hyprland. O daemon exige que essa opção esteja
   vazia (mapeamento por todo o layout).
3. **Dois dispositivos virtuais**: stylus (`ABS_X/Y/PRESSURE`, `BTN_TOOL_PEN`,
   `BTN_TOOL_RUBBER`, `BTN_TOUCH`, `BTN_STYLUS/2`) e teclado/mouse de macros
   (teclas, botões, REL_WHEEL/HWHEEL).
4. **`INPUT_PROP_POINTER`, não `DIRECT`**: a CTL-4100 não é uma tela; `DIRECT`
   faria o libinput tratá-la como touchscreen e quebraria o mapeamento.
5. **Perfil por app** escutando `.socket2.sock` (`activewindow>>classe,título`)
   e lendo `hyprctl monitors -j` a cada 5 s.
6. **Ações internas** (precisão, troca de monitor, radial) ficam no daemon;
   teclas/cliques/shell vão para o dispositivo de macros.

---

## 6. Integração Arch / Omarchy

- **udev**: desvincula `wacom` do kernel só para `056A:0374/0376/03C5`,
  libera hidraw para o grupo `input` e uinput para o grupo `input`.
- **systemd --user**: `wacomd.service` em `graphical-session.target`.
- **módulo**: `/etc/modules-load.d/wacom-ctl4100.conf` com `uinput`.
- **Hyprland**: monitores via `hyprctl monitors -j`; foco via socket2;
  `active_area`/rotação são feitos no daemon, não no compositor.
- **PKGBUILD**: compila o workspace, instala binários, regra, unit, desktop,
  licença e documentação.

---

## 7. Limites, incertezas e o que não foi verificado

1. **Sem hardware nesta sessão.** O descritor veio de um dump público de
   hardware real (linuxwacom/wacom-hid-descriptors, contribuição de 2018 e
   2020); os comandos de feature vieram do binário oficial e do kernel Linux.
   Não houve captura `evtest`/`hidraw` local.
2. **Presets de pressão**: arquitetura Bézier + LUT confirmada; pontos exatos
   dos presets Soft/Firm não recuperados.
3. **Duplo clique**: distância/tempo padrão são nossos (10 px / 400 ms).
4. **Tilt**: o caminho OAK do driver oficial não usa tilt; nosso parser lê os
   campos, mas não os emite no uinput.
5. **Eraser**: bit vendor `0x36`; a LP-1100K não tem borracha. Se esse bit
   tiver outro significado em firmware diferente, o comportamento muda.
6. **Bluetooth**: o modo BT usa pacotes de 361 bytes e feature `0x82`; não
   implementado. O projeto cobre USB.
7. **`0xAC` raw**: observado, não implementado.
8. **Mapeamento**: a matemática é nossa; o driver oficial usa `SMatrix` e
   opções de "inset tablet mode" que não foram bit-a-bit reproduzidas.
9. **`input:tablet:output` do Hyprland**: se estiver configurado, o
   compositor reescala o dispositivo e o mapeamento do daemon fica relativo.
   Deixe vazio.
10. **Desempenho**: o daemon lê com timeout de 5 ms e publica telemetria a
    20 Hz; a taxa real da mesa é limitada pelo firmware.

---

## 8. Evidências reproduzíveis

Comandos usados (a partir de `re-workspace/`, ignorado pelo Git):

```bash
7z x WacomTablet_6.4.15-2.dmg
xar -xf InstallWacomTablet.pkg
gunzip -c content.pkg/Payload > payload.cpio
cpio -idm --no-absolute-filenames < payload.cpio

llvm-lipo WacomTabletDriver -thin x86_64 -output WacomTabletDriver.x86_64
llvm-nm --arch=x86_64 -C --defined-only WacomTabletDriver.x86_64 | wc -l
llvm-objdump -d --demangle --start-address=0x1002e0416 \
  --stop-address=0x1002e04c0 WacomTabletDriver.x86_64   # SetToWacomMode

/opt/ghidra/support/analyzeHeadless /tmp/opencode/ghidra_wacom WacomProj \
  -import WacomTabletDriver.x86_64 -scriptPath ghidra_scripts \
  -postScript DumpDecomp.java out.c CCTL4100 CGD16 CPressureCurve
```

Símbolos/endereços citados (slice x86_64):

| Endereço | Símbolo |
| ---: | --- |
| `0x1000DC15C` | `CCTL4100Pen::Init` |
| `0x1000DC39A` | `CCTL4100Pen::HandleTransducerData` |
| `0x1000DFC62` | `CCTL4100GraphicsTablet::Init` |
| `0x1000DFEBE` | `CCTL4100GraphicsTablet::DeviceStart` |
| `0x1000E02C2` | `CCTL4100GraphicsTablet::ProcessDataPacket` |
| `0x1002E0364` | `MacHidDevice::SetToHIDMode` |
| `0x1002E0416` | `MacHidDevice::SetToWacomMode` |
| `0x100397C9E` | `CUSBCommInterface::SetToHIDMode` |
| `0x100399302` | `CUSBCommInterface::GD16SetScanModePen` |
| `0x10039939C` | `CUSBCommInterface::GD16SetScanModeTouch` |
| `0x100399DDE` | `CUSBCommInterface::GD16SendHardwareReset` |
| `0x1001AABF6` | `CPressureCurve::CPressureCurve` |
| `0x1001AADD2` | `CPressureCurve::EvaluateAtValue` |
| `0x1001AAE28` | `CPressureCurve::EvaluateAll` |
| `0x10023C642` | `CGD16StylusTransducer::HandleTransducerData` |
| `0x1003E80DE` | `CCTL4100TabletControls::ProcessCurrentTagList` |
| `0x10040CFD9` | `HidReport::TranslateShortEMR` |
| `0x10003731EE` | `CTransducer::CheckDoubleClickAssist` |
| `0x100310E58` | `CMappingOutputScreen::GetMonitorArea` |

Descritor HID e dumps auxiliares ficam em `re-workspace/kernel-ref/` e
`/tmp/opencode/wacom-hid-desc/` (fora do Git). A cópia de trabalho do
`wacom-hid-descriptors` é GPL-2.0; apenas os fatos do descritor são citados
aqui, não o arquivo.
