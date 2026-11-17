# Audio: Windows baseline and gaps on Linux

This covers the Realtek ALC274 HDA codec (`10ec:0274`, codec SSID `10ec:1284`) and the Intel SST controller (`8086:51c8`, PCI SSID `8086:7270`).
The Linux reference is v7.3.4 and linux-surface `patches/7.3`. Windows driver files are named by their driver package; they live under
`C:\Windows\System32\DriverStore\FileRepository\`.

Results measured on Linux 7.3.4 are in 2.1, in the "On Linux" column of section 3 and in section 4. The acoustic figures come from recording the built-in speakers with the built-in microphones, so they are
coarse, and there is not a single listening judgement.

ACPI objects are named by path; they are in the DSDT. The ACPI tables are not in this repository: dump and decompile them on your own device with
[`tools/windows/dump-acpi.ps1`](../../../tools/windows/dump-acpi.ps1), or take them from <https://github.com/linux-surface/acpidumps> (directory `surface_pro_9_intel`).
`#N` is an issue of linux-surface/linux-surface.

## 1. Windows baseline

### 1.1 Devices and driver path

| Item | Value | Evidence |
|---|---|---|
| Controller | `PCI\VEN_8086&DEV_51C8&SUBSYS_72708086`, class `0x040380` (DSP or legacy mode) | CompatIds `CC_040380` in `pnp-devices.csv` |
| Bus/DSP driver | IntcAudioBus (SST bus) + IntcOED (SST DSP, `DSP_CTLR_DEV_51C8&DEV_0222`) | `driver-packages.txt` |
| DSP firmware | `Fw\dsp_fw_adl_release.bin` (653444 B), libraries `dsp_lib_intel_wov_adl`, `dsp_lib_whm_adl`, topology `IntcOEDTopology.xml/.oec`, a number of WoV models | Same file. Names and sizes only |
| Codec driver | Realtek 6.0.9681.3, `ehdxsstmd3a4.inf`, name "Surface High Definition Audio" | INF |
| Stream path | The codec streams are routed through the SST DSP (the INF sets `ForceSST` = 1 under `Settings`; endpoint topologies `RearLineOutWaveSST`, `RtMicInSSTTopo`) | INF. Registry `SSTCus\RearLineOutWaveSST = "Support offload!"` |
| DSP extras | BT audio offload (IntcBTAu, `LINKTYPE_03`, `DEV_AE30`), USB audio offload (IntcUSB, `LINKTYPE_06`, `DEV_AE50`), voice wake word (WoV, `SSTCus\WOVMultiPA=1`) | `pnp-devices.csv`, driver key `SSTCus` |
| Speaker amplifier | ACPI has no I2C/SPI amplifier (`CSC`, `TIAS`, `MX98`, `ESSX` and so on). No smart amplifier (feedback IV sensing) | `acpi-devices.txt`; no match in the decompiled ACPI tables |

`pnp-devices.csv`, `driver-packages.txt` and `acpi-devices.txt` are the Windows-side inventories of this device (`docs/reference/`).

The driver key is `HKLM\SYSTEM\CurrentControlSet\Control\Class\{4d36e96c-...}\0002`.
`Settings\AIODatTableIndex = 39`, `Settings\Drv9681_DevType_0274_SS10ec1284\Pin12 = 1`, `GlobalSettings\MicEQ = 0`,
`EQVol\EnabledBy274Coe82 = 7`, `CSP\fixedCTIAheadset = 1`, `headsetMicPlugged = 0`.

### 1.2 Codec verb table (SSID `10ec:1284`)

At initialisation the Realtek driver applies the per-SSID block of its table data file `RTAIODAT.DAT` (in the `ehdxsstmd3a4.inf` package).
The file's SSID index has an entry for `10ec1284` whose number is `0x27` (=39, the same as `AIODatTableIndex` in the registry).
The block for this SSID is 1280 B long and is stored in the clear.

Pin default configuration:

| NID | Value | Meaning |
|---|---|---|
| `0x12` | `0xb7a60120` | Fixed, location `0x37` (other/inside mobile lid), Mic In, connection Other Digital, assoc 2 seq 0. **The codec DMIC input** |
| `0x13` | `0x40000000` | Port connectivity 01 (no physical connection) |
| `0x14`~`0x1a` | `0x411111f0` | Not connected |
| `0x1b` | `0x90170110` | Fixed, internal, Speaker, Other Analog, assoc 1 seq 0. **The speaker is on 0x1b, not on 0x14/0x17** |
| `0x1d` | `0x4063002d` | Realtek customisation value (checksum valid, low byte `0x2d`) |
| `0x1e`, `0x1f`, `0x21` | `0x411111f0` | Not connected. **There is no headphone pin (0x21) and no headset microphone (0x19)** |

The remaining verbs (all `SET_COEF_INDEX`/`SET_PROC_COEF` pairs):

| NID | Index=value |
|---|---|
| `0x20` (vendor coefficients) | `35=8968`, `6f=2c0b`, `75=2050`, `33=8601`, `34=a23c`, `4a=a01b`, `0d=a420`, `0c=7c01`, `08=a380`, `0a=7772`, `0b=7772`, `0f=7772`, `43=0000`, `3b=8000`, `40=0800`, `06=6510` |
| `0x58` | `00=3888` |
| `0x57` | `06=60a3` |
| `0x53` | `02=1000`, `00=000a`, `1d=0800`, `1e=047f`, `02=1805`, and last `00=c00a` |
| `0x54` | `00=0000`, `1d=0800`, `1e=04c3` |

`0x53`/`0x54` are a left/right pair that use the same indices; only `1e` differs, `047f` against `04c3`. The final `0x53 idx0 = c00a` is a write that sets the top 2 bits.
The driver key `EQVol` holds `Saved_53_00 = 0x400a`, `Saved_53_1d = 0x0800`, `Saved_54_1d = 0x0800` and `EnabledBy274Coe82 = 7`, and
the `VerbInfo` error record holds `0x05350000`/`0x0534400A` (coefficient writes to NID 0x53). These nodes are a processing block inside the codec (it appears to be per-channel gain and EQ; inferred).
The block contains no GPIO verbs (`0x715`~`0x717`).

The same block is followed by a microphone EQ table, but `GlobalSettings\MicEQ = 0` (it appears to be inactive; inferred).
Blocks with the same pin layout (DMIC on 0x12, speaker on 0x1b, no jack) also exist for SSIDs `10ec:12d2` and `10ec:1332`.

### 1.3 Endpoints

In `MMDevices\Audio` only two endpoints are active (DeviceState 1). The other two are Intel display audio (no monitor connected, state 4).
The jack descriptions were read through Core Audio `IKsJackDescription`/`IKsJackDescription2`.

| Endpoint | Form | Device format | Jack description |
|---|---|---|---|
| Surface Omnisonic speakers (`RearLineOutWaveSST`) | FormFactor 1 (Speakers), `KSNODETYPE_SPEAKER` | 2ch 48 kHz 16-bit, channel mask `0x3` (FL+FR). Engine mix format float32 2ch | ChannelMapping `0x3`, ConnType 7 (Other Analog), GeoLoc 14 (N/A), GenLoc 0, PortConn 1 (integrated) |
| Surface stereo microphones (`RtMicInWave`, KS pin "Microphone Array") | FormFactor 4, `KSNODETYPE_MICROPHONE_ARRAY` | 2ch 48 kHz 16-bit. Communications-mode processing format 1ch float 48 kHz/16 kHz | ConnType 6 (Other Digital), GeoLoc 9 (inside mobile lid), GenLoc 3 (other), PortConn 1 |

There is no headphone or headset endpoint, not even in the unplugged state. This agrees with the SP9 having no 3.5 mm jack and with the pin table (0x19/0x21 N/A).

Microphone array geometry (the microphone array geometry property of `KSPROPSETID_Audio`, topology filter `rtmicintopo` pin 1):
version `0x100`, linear array, 2 microphones (type 1, subcardioid), coordinates x=0, y=±21 mm, z=98 mm (spacing 42 mm),
vertical and horizontal angle range ±0.7853 rad (±45 degrees) each, frequency 300–8000 Hz.

### 1.4 NHLT

The NHLT ACPI table is 753 B. iasl does not decode its body (it emits a raw dump only), so the table below was decoded by hand.

| EP | Link | Device | Direction | Virtual bus | Formats |
|---|---|---|---|---|---|
| 0 | SSP(3) | `8086:AE30`, device type 0 (BT) | render | 2 | 8 kHz/16 kHz mono 16-bit, 48 kHz stereo 16-bit |
| 1 | SSP(3) | `8086:AE30`, device type 0 (BT) | capture | 2 | 8 kHz/16 kHz mono 16-bit |

**There is no PDM (DMIC) endpoint.** The microphones are attached to the codec's DMIC pin (0x12), not to a DMIC link of the DSP. A 4 B OED configuration follows at the end of the NHLT.
The DSDT device `HDAS` has `_DSD "nhlt-version" = "1.8-0"` and passes the NHLT location (`NHLA`/`NHLL`) through `_DSM` (UUID `a69f886e-...`).
`HDAS.IDA.SNDW` exists, and its `_INI` fills `mipi-sdw-master-count` from `SWMC` in the NVS and the per-link `intel-quirk-mask` from `SWQ0`~`SWQ3`. These are runtime NVS values and cannot be known from a static dump.

### 1.5 APOs (software processing)

The actual chain per endpoint was read from `MMDevices\Audio\...\FxProperties`.

| Endpoint | Stage | APO | File |
|---|---|---|---|
| Speakers | SFX, OSFX | Dolby DAX3 wrapper `{0EBD8505-...}`, `{0EBD8511-...}` | `DolbyDax3Apo.dll` (package `dax3_swc_aposvc`) |
| Speakers | MFX, OMFX | Microsoft Surface Render APO - MFX `{34D30CD8-...}` | `SurfaceAPO.dll` (package `surfaceaposwcinstaller`) |
| Speakers | EFX | Microsoft Surface Render APO - EFX `{501c3309-...}` | Same DLL |
| Microphones | MFX | Surface Capture APO - MFX `{25C32FEE-...}` (includes AEC) | `SurfaceCaptureAPO.dll` (package `surfacecaptureaposwcinstaller`) |
| Microphones | EFX | Realtek RtkRecEFX `{AB3B404A-...}` | `RltkAPOU64.dll` (package `realtekapo`) |

Voice Clarity (`voiceclaritycpuapo.dll`) is installed but is not in the chains above. The INF puts
`PKEY_Devices_AudioDevice_Microphone_SpeakerCoupling` on the microphone endpoint.

**The Surface Render EFX tuning** is `SurfaceAPO_1284.json` in the `surfaceapoextension.inf` package (JSON, 48 kHz, same values left and right).

| Block | Values |
|---|---|
| `MainEQ` | 10 of 16 biquads in use (5 per channel): 2 HPF stages (each -3 dB ≈ 50 Hz, together -32 dB at 20 Hz), peak -9.0 dB @ 3763 Hz Q≈2.0, peak -3.0 dB @ 2502 Hz Q≈1.5, high boost +3.7 dB @ 10.5 kHz Q≈0.7 (+3.0 at 20 kHz). Combined response: 60 Hz -3.4, 1 kHz -0.5, 3 kHz -7.4, 5 kHz -4.8, 10 kHz +3.3 dB |
| `VolumeDepLS` | Volume-dependent low shelf. +1.5 dB for each -2.75 dB volume step, +17.5 dB at the lowest (-24.75 dB and below), centre 440 → 150 Hz |
| `VolumeDepHS` | Volume-dependent high shelf +1 → +10 dB, centre ≈ 6.5 kHz |
| `VolumeDepMBDRC4` | 4-band DRC, crossovers 100/600/2750 Hz. Bands 1 and 2: threshold -6/-5 dB, ratio 2.0/1.7, pre-gain +4 dB; bands 3 and 4 pass through. attack 1 ms, hold 80/80/100/80 ms, release 20/10/10/20 ms |
| `Crystal` | Dynamic limiting per resonance band: 160/240/455/644/844 Hz, bandwidth 270/120/250/295/300 Hz, limit -2.7/-12.0/-18.5/-15.1/-14.7 dB |
| `OutputLimiter` | 0 dBFS, lookahead 10 ms, attack 2 ms, release 10 ms |
| Per-mode MFX EQ | DEFAULT/RAW/COMMS/MOVIE/MEDIA are identity. Only NOTIFICATION has -6 dB @ 456 Hz |

**The Dolby tuning** is `DEV_0274_SUBSYS_10EC1284_PCI_SUBSYS_72708086.xml` in the `dax3_ext_rtk.inf` package.
`internal_speaker`, 2 front, no subwoofer. Dynamic profile: dialog enhancer 5, volume leveler on, surround decoder/virtualizer on,
`volmax-boost 96`. audio-optimizer, IEQ and graphic EQ are off; the regulator is on, but its thresholds are the 0/-192 preset, so it carries no per-band protection values.
Speaker correction and protection are done by the Surface APO, not by Dolby.

## 2. State of Linux

### 2.1 Driver selection (`snd_intel_dspcfg`)

`sound/hda/core/intel-dsp-config.c` has three entries for `0x51c8` (ADL-P) (lines 423-446).
It is not a Chromebook DMI (425), there is no ES83x6 codec HID (438), and the third (443) is `FLAG_SOF | FLAG_SOF_ONLY_IF_DMIC_OR_SOUNDWIRE`.

- DMIC condition: false, because the NHLT has no PDM endpoint (`snd_intel_dsp_check_dmic`, line 643).
- SoundWire condition: `sound/hda/core/intel-sdw-acpi.c` finds `SNDW` (`_ADR 0x40000000`), creates links from `mipi-sdw-master-count` (line 86), and
  enables a link if its `intel-quirk-mask` does not have bit1 (BUS_DISABLE) (lines 49, 128). If `SWMC > 0` and `SWQx` has no bit1 the result is SOF (line 731), otherwise legacy (line 755).
  These are NVS values and could not be determined from Windows. The Surface Laptop 5, with the same ADL `8086:51c8`, comes up as `sof-audio-pci-intel-tgl`
  ([linux-surface#1010](https://github.com/linux-surface/linux-surface/issues/1010)), and the RPL Surface Laptop Studio 2 (ALC274) is also SOF + `skl_hda_dsp_generic`
  ([#2203](https://github.com/linux-surface/linux-surface/issues/2203)).

**This device comes up with legacy `snd_hda_intel` (measured).** The driver in `lspci -nnk -s 00:1f.3` is `snd_hda_intel`, `dsp_driver=0` (auto), and the kernel command line
has no `dsp_driver`. There is one card, `HDA Intel PCH` (ALC274 Analog + 4 HDMI), and the kernel log has no SoundWire, DMIC or sof-audio line.
No SOF firmware or topology is loaded. That other Surface devices with the same PCI ID come up with SOF does not carry over to the SP9.
The cause is inferred: the log has none of the lines `sdw_intel_scan_controller` prints for a link count of 0 or a missing property, which leaves the case where the link count is greater than 0 but every link's
`intel-quirk-mask` has BUS_DISABLE (bit1) set, giving `link_mask = 0`. The NVS values `SWMC` and `SWQx` were not read
(`/dev/mem` is blocked by `CONFIG_STRICT_DEVMEM`).

### 2.2 When the path is SOF

This does not apply to this device (2.1). It is kept for firmware that does come up with SOF.

`hda_generic_machine_select` (line 1039) in `sound/soc/sof/intel/hda.c` picks
`skl_hda_dsp_generic` and `sof-hda-generic` (`sound/soc/intel/common/soc-acpi-intel-hda-match.c` lines 15-17).
The codecs are ALC274 + 2 HDMI, so no SoundWire link is enabled (lines 1110-1115). `check_dmic_num` (line 560) is 0, so no `-Nch` suffix is added.
The firmware is `intel/sof/sof-adl.ri` and the topology `intel/sof-tplg/sof-hda-generic.tplg` (IPC3 by default, `pci-tgl.c` lines 163-177).
Because of BT SSP2 in the NHLT, `bt_link_mask = 0x4` is passed to the machine driver (`hda.c` line 1544, `skl_hda_dsp_generic.c` lines 77-82) and
a BT offload DAI link is created, but the generic topology has no pipeline for it (impact inferred: warning level). Linux BT audio takes the HCI (btusb) path and works without offload.
The UCM is `ucm2/Intel/sof-hda-dsp/HiFi.conf` → `HDA/HiFi-analog.conf`, `HDA/HiFi-mic.conf`.

### 2.3 When the path is legacy (`snd_hda_intel`)

The DSP is not used and the codec is driven directly over the HDA link. The NHLT has no DMIC, so no input is lost. The UCM is the `HDA-Intel` profile.
This is the path of this device. It is also the path on which `snd_hda_intel patch=` applies directly (loading a patch file was not tested).

### 2.4 Codec driver (common to both paths)

`sound/hda/codecs/realtek/alc269.c`:

- In `alc269_probe` (line 9195), `0x10ec0274` is `ALC269_TYPE_ALC294`, `mixer_nid = 0`, an update of coefficient `0x6b`, `init_hook = alc294_init` (lines 9325-9331).
- `snd_hda_pick_fixup` (line 9361; implemented in `sound/hda/common/auto_parser.c` lines 962-1050) looks at the PCI SSID `8086:7270` first (line 1022) and then at the codec SSID `10ec:1284` (line 1038).
  `alc269_fixup_tbl` (line 7213) has neither `8086:7270` nor `10ec:1284`. The only `10ec:` entries are `10f2`, `111e`, `118c`, `119e`, `11bc`, `1230`, `124c`, `1252`, `1254`, `12cc`, `12f6` (lines 8033-8043).
  `8086:7270` appears only in `alc262.c` line 110 (BayleyBay) and in the Cirrus table (MacBookAir7,2).
- The ALC274 entries of the pin quirk table are HP (`0x103c`, line 8888) and Dell (`0x1028`, line 9133) only, and the vendor table (line 8535) has no `10ec`/`8086`/`1414` either.
- Result: **no fixup**. The generic parser builds the paths from the pin defaults the BIOS left. The coefficient writes of 1.2 are not made.
- The only audio patch in linux-surface `patches/7.3` is `0002-surface3.patch` (Surface 3 RT5645).

## 3. Gaps

In the "On Linux" column, "Check" is the command or procedure and "Measured" is what it gave on this device.

| # | Gap | Evidence | Impact | How to close it | Confidence | On Linux |
|---|---|---|---|---|---|---|
| G1 | The DSP path cannot be determined from the Windows side. On SOF, the sibling SL5 hangs on the second s2idle entry (1f.3 stays in D0) | 2.1. #1010 | Audio may play while suspend stalls | If the device comes up with SOF and the suspend problem reproduces, `snd_intel_dspcfg.dsp_driver=1` (legacy). The NHLT has no DMIC, so no function is lost (BT and USB offload and WoV do not exist on Linux anyway) | Path: inferred. Nothing lost on legacy: confirmed (NHLT) | Check: `dmesg \| grep -iE 'dspcfg\|SoundWire enabled\|Digital mics\|sof-audio'`, `lspci -k -s 00:1f.3`, `rtcwake -m freeze -s 20` twice. Measured: legacy `snd_hda_intel` (2.1). This problem occurs only on SOF and does not apply to this device. When idle, `00:1f.3` is `runtime_status: suspended`, `D3hot`. After playback, 3 consecutive runs of `rtcwake -m freeze -s 20` enter and resume 3/3 and playback is normal afterwards (PWR-3) |
| G2 | No quirk for codec SSID `10ec:1284`. The pin configuration depends on the BIOS defaults. Windows rewrites the pins from the table in 1.2 | 2.4, 1.2 | If the BIOS values differ from the table, a headphone/headset port that does not exist shows up (0x19/0x21) or the speaker and DMIC are detected wrongly | If there is a difference, put `[codec] 0x10ec0274 0x10ec1284 0` + `[pincfg]` (the table in 1.2) in `/lib/firmware/sp9-alc274.fw` and set `options snd-hda-intel patch=sp9-alc274.fw` (legacy path). On the kernel side, `HDA_CODEC_QUIRK(0x10ec, 0x1284, "Microsoft Surface Pro 9", ALC274_FIXUP_SURFACE_PRO_9)` (matching on the codec SSID is required: the PCI SSID `8086:7270` is a generic Intel value) + a `HDA_FIXUP_PINS` table | Inferred (BIOS values unverified) | Check: compare `cat /sys/class/sound/hwC*D0/init_pin_configs` with the table in 1.2. `dmesg \| grep -i 'ALC274\|autoconfig\|picked fixup'`. Measured: all 14 `init_pin_configs` equal the table in 1.2 (0x12 `b7a60120`, 0x13 `40000000`, 0x1b `90170110`, 0x1d `4063002d`, the rest `411111f0`). `driver_pin_configs` and `user_pin_configs` are empty and there is no fixup. autoconfig gives `line_outs=1 (0x1b) type:speaker`, `Mic=0x12`. No pin patch is needed |
| G3 | Linux does not make the coefficient writes of 1.2 (NID 0x20/0x53/0x54/0x57/0x58) | 1.2, 2.4 | If 0x53/0x54 (the left/right processing block, whose `1e` value differs between left and right) is off or at its default, left/right level and tone may differ from Windows. The 0x20 coefficients appear to be analog bias, DMIC and class settings, and leaving them out may cause noise or DMIC clock problems | Put the 56 verb lines that correspond to the table in 1.2 (`0x20 0x500 0x0035` / `0x20 0x400 0x8968` …) under `[verb]` in the patch file. When moving this to the kernel, write a `struct coef_fw` array (`WRITE_COEFEX(0x53, 0x1e, 0x047f)` and so on) in `HDA_FIXUP_ACT_INIT`. Do not add everything at once; A/B compare starting with 0x53/0x54 | Inferred (effect not measured) | Check: `hda-verb /dev/snd/hwC0D0 0x53 SET_COEF_INDEX 0x1e`, then `hda-verb ... 0x53 GET_PROC_COEF 0`, and compare the current value with 1.2 (0x00, 0x02, 0x1d, 0x1e); the same for 0x54. Not done: reading a coefficient needs a `SET_COEF_INDEX` write first |
| G4 | No speaker correction or protection processing. Windows always applies the Surface Render EFX (HPF 50 Hz, cuts at 3.76 kHz/2.5 kHz, high shelf, volume-dependent loudness, 4-band DRC, resonance limiting, limiter). With no smart amplifier, this software is all the protection there is | 1.5, the amplifier row of 1.1 | Excess excursion in the lows (no attenuation below 50 Hz), distortion and speaker stress at the 160–850 Hz resonances, and 2.5–4 kHz sounding harsh | A speaker-only sink with PipeWire filter-chain (or EasyEffects): 2 stages of `bq_highpass` 50 Hz Q 0.707, `bq_peaking` 3763 Hz -9 dB Q 2.0, `bq_peaking` 2502 Hz -3 dB Q 1.5, `bq_highshelf` ≈10 kHz +3 dB, output limiter -1 dBFS (lookahead 10 ms). Optional: 4-band compression (100/600/2750 Hz, lower 2 bands -6/-5 dB, 2:1/1.7:1), dynamic EQ at 160/240/455/644/844 Hz. The volume-dependent loudness can be left out | Windows values: confirmed. Effect on Linux: inferred | Check: listen to `speaker-test -c2 -t sine -f 40/100/3700` before and after the filter, compare distortion at maximum volume, load in `pw-top`. Measured (this repository's filter chain): the digital transfer is 20 Hz −27.9, 50 Hz −2.0, 100–300 Hz +3.5~+3.9, 1 kHz +0.1, 3763 Hz −9.4, 5 kHz −2.9, 10 kHz +3.3 dB. Acoustic (volume 100 %, −1 dBFS, raw → filtered): 40 Hz fundamental −62.8 → −69.3 dBFS, 3.7 kHz THD 16.9 → 4.7 %. **Full-scale tones at 100 Hz, 400 Hz and 1 kHz have the same level and THD after the filter** (100 Hz THD 72 → 69 %): after the +4 dB pre-gain of bands 1 and 2, compression and the limiter bring the level back to that of raw. `Crystal`, which holds this range down on Windows, was not ported. This filter chain therefore does not protect the 100 Hz–1 kHz range at maximum volume. Of the combined-response figures in 1.5, those for 3 kHz, 5 kHz and 20 kHz differ from the measured and calculated values of this repository's configuration (−6.6, −2.9, +0.4 dB). That this is because the Q and frequency values in 1.5 are approximations and the 10.5 kHz term has a shelf shape is inferred. The load is about 1.2 % of the period. The raw sink must be at 100 %: left at WirePlumber's default of 40 %, the amplifier sits at −23.9 dB even with the filter sink at 100 % (measured). The platform files raise the raw sink to 100 % every time WirePlumber starts ([platform files](../../../userspace/platform/README.md), the section on speaker processing). How it sounds is unverified (AUD-1, AUD-2) |
| G5 | The microphones deliver only the raw 2-channel DMIC signal. Windows applies the Surface Capture APO (AEC/beamforming) and the Realtek recording EFX | 1.3, 1.5 | Echo and noise in calls. On the SLS2 (same codec, same DMIC on 0x12) stereo phase cancellation and Mic Boost clipping are reported (#2203) | Check the phase first. If one side is inverted: `options snd-hda-intel model=inv-dmic` (legacy, card 0), or `[model]` in the patch file, or an `ALC269_FIXUP_INV_DMIC` chain. Default Mic Boost 0 dB. Create a virtual source with PipeWire `module-echo-cancel` (webrtc, AEC+NS) and downmix to mono. If beamforming geometry is needed, use the coordinates in 1.3 (y=±21 mm) | No processing: confirmed. Phase inversion: no (same polarity in a Windows recording of ambient noise, sum−difference +9–11 dB. That value went through the APO path; the raw signal on Linux is in the next column) | Check: `arecord -D hw:0,0 -c2 -r48000 -f S16_LE -d 5 m.wav`, compare the RMS of `sox m.wav -n remix 1,2 stat` and `remix 1,2v-1 stat` (inverted if the difference signal is larger than the sum when speaking from the front), `amixer -c0 contents \| grep -i boost`. Measured: signal on both channels, level difference 0.6~0.8 dB. The polarity is the same (with both speakers playing at once, sum−difference +10.9 dB, correlation +0.88; by the sox method, sum RMS 0.0336 > difference 0.0235). `model=inv-dmic` is not needed. It was not measured with speech from the front. The default state is `Capture` +30 dB + `Mic Boost` +30 dB (source volume 100 %), and it clips there (155 samples on ch0 and 96 on ch1 in 2 seconds of a 1 kHz tone). At `Mic Boost` steps 0–2 the count is 0. When capture is opened as the codec wakes from runtime suspend, the first 1.0 second is stuck at full scale. The cause of this transient is unverified (AUD-3). The platform files contain a WirePlumber rule that sets the default gain to +30 dB (`Capture` +30 dB, `Mic Boost` 0 dB); it applies only to accounts with no stored value. They also contain an echo-cancel source `sp9_echo_cancel`: the source appears and all its nodes are suspended when it is not in use (measured). Call quality is unverified (AUD-4) |
| G6 | On a device without a jack, if pins 0x19/0x21 are not N/A in the BIOS, UCM creates headphone and headset ports. Conversely, if the internal microphone is tied to a jack control it is hidden as "unavailable" (the SLS2 case) | 1.2, 1.3, #2203 | Automatic output switching misbehaves, the microphone is not visible in applications | Solved by the pin patch of G2 (0x19/0x21 = `0x411111f0`). Change the UCM only if it remains after that | Inferred | Check: `wpctl status`, the ports and `available` values in `pactl list sinks/sources`, `amixer -c0 controls \| grep -i jack`. Measured: no headphone or headset port. Sink port `analog-output-speaker`, source port `analog-input-mic`, both with availability unknown (selectable). The only jack controls are `Speaker Phantom Jack` and the 4 HDMI ones |
| G7 | On the SOF path a BT offload DAI is created but is not in the generic topology | 2.2 | A warning or a DAI link failure in the log (inferred). BT audio itself takes the btusb path | If a problem shows in the log, `snd_sof_intel_hda_generic.bt_link_mask=0` (module parameter `bt_link_mask`, `hda.c` lines 499-501) | Inferred | Check: `dmesg \| grep -i 'BT link\|ssp2\|BT_OFFLOAD'`. Not applicable (legacy path, 2.1) |
| G8 | The DSP extras (USB audio offload, WoV wake-word detection, DSP-side AEC) do not exist on Linux | 1.1 | A difference in features only; the paths in use exist (USB audio = snd-usb-audio, voice = software) | Nothing to do | Confirmed (driver set) | Not applicable |

## 4. Checks on Linux and their results

Each item is the check, followed by its result on Linux 7.3.4.

1. Path: `dmesg | grep -iE 'snd_intel_dspcfg|SoundWire|DMICs detected|BT link|hda codecs found|sof-audio|skl_hda|ALC274|autoconfig|picked fixup'`,
   `cat /sys/module/snd_intel_dspcfg/parameters/dsp_driver`, `aplay -l`, `arecord -l`. With SOF the firmware and topology file names are printed in the log.
   Result (measured): legacy HDA (2.1). PCM `0,0` (ALC274 Analog playback and capture) + 4 HDMI. 0 audio-related error/warn lines.
2. Keeping a codec dump: `cat /proc/asound/card*/codec#0 > codec-alc274.txt` (it holds no values unique to a unit), `init_pin_configs`, `driver_pin_configs`, `subsystem_id` (`0x10ec1284` expected).
   Compare the pin values with the table in 1.2 to decide G2/G6.
   Result (measured): vendor `0x10ec0274`, subsystem `0x10ec1284`, PCI SSID `8086:7270`. For the pin values see G2.
3. Coefficient comparison (G3): read each index of 1.2 with `hda-verb` (from `alsa-tools`) and record the entries that differ from the Windows values.
   Result: not done (G3).
4. Speakers: left/right position with `speaker-test -c2 -t wav`, the EAPD state (`EAPD` of Node 0x1b in `codec#0`), distortion over volume steps 0–100%. If there is no sound, look at the 0x1b EAPD and the G3 coefficients first.
   Result (measured): 0x1b `EAPD 0x2`, `Pin-ctls 0x40 OUT`. The left and right channels each produce sound, are on opposite sides, and have the same polarity. However, playback L reaches capture ch1 first and louder.
   Whether the speakers or the microphone channels are swapped is unverified. The THD of a −1 dBFS tone on the filter path, at 1 kHz, is 0.4/1.4/2.5/3.1 % at volume 25/50/75/100 % (AUD-1).
5. Compare the same material before and after the filter chain (G4) and see whether low-frequency distortion at maximum volume is reduced.
   Result (measured): see G4.
6. Microphones: 2-channel recording, level per channel, phase (the sox comparison of G5), clipping per Mic Boost step, a call application on the echo-cancel source.
   Result (measured): see G5.
7. Port display (G6): `wpctl status` must show no headphone or headset port and the internal microphone must be `available`.
   Result (measured): see G6.
8. Suspend (G1): `rtcwake -m freeze -s 20` at least 2 times in a row, with `cat /sys/bus/pci/devices/0000:00:1f.3/power/runtime_status` before each. If it hangs on SOF, the same test with `dsp_driver=1`.
   Result (measured): see G1 (PWR-3 pass).
9. BT audio (A2DP playback, HFP microphone), HDMI audio playback on a USB-C/DP monitor.
   Result: unverified (AUD-5, AUD-6). The A2DP and HFP endpoints are registered with BlueZ and there are 4 HDMI/DP PCMs. No paired device and no connected monitor were available.
10. Keeping the basis of the path decision: the master-count and quirk-mask of `SNDW` are runtime NVS values, so record the SoundWire lines of `dmesg` and whether an SNDW device exists under `/sys/bus/acpi/devices/`.
    Result (measured): ACPI `\_SB_.PC00.HDAS.IDA_.SNDW` exists (status 15). The `SWMC` and `SWQ0-3` values were not read (2.1).
