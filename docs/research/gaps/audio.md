# Audio: Windows baseline and gaps on Linux

In short: this device comes up on the legacy `snd_hda_intel` path with no codec fixup, and the BIOS pin values are already
right, so speakers and microphones work without a pin patch. What Linux lacks is Windows' software processing: the
speaker processing (a PipeWire filter chain built from this device's own measurements stands in, G4) and the capture processing
(an echo-cancel source is provided, G5).

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
| Stream path | The codec streams are routed through the SST DSP (endpoint topologies `RearLineOutWaveSST`, `RtMicInSSTTopo`, offload supported) | Endpoint names, `SSTCus` driver key |
| DSP extras | BT audio offload (IntcBTAu, `LINKTYPE_03`, `DEV_AE30`), USB audio offload (IntcUSB, `LINKTYPE_06`, `DEV_AE50`), voice wake word (WoV) | `pnp-devices.csv` |
| Speaker amplifier | ACPI has no I2C/SPI amplifier (`CSC`, `TIAS`, `MX98`, `ESSX` and so on). No smart amplifier (feedback IV sensing) | `acpi-devices.txt`; no match in the decompiled ACPI tables |

`pnp-devices.csv`, `driver-packages.txt` and `acpi-devices.txt` are the Windows-side inventories of this device (`docs/reference/`).

### 1.2 Codec pin configuration (SSID `10ec:1284`)

Pin default configuration, read on the device (`/sys/class/sound/hwC0D0/init_pin_configs`, gap G2). It describes the hardware as the firmware leaves it, and the Windows endpoints in 1.3 agree with it:

| NID | Value | Meaning |
|---|---|---|
| `0x12` | `0xb7a60120` | Fixed, location `0x37` (other/inside mobile lid), Mic In, connection Other Digital, assoc 2 seq 0. **The codec DMIC input** |
| `0x13` | `0x40000000` | Port connectivity 01 (no physical connection) |
| `0x14`~`0x1a` | `0x411111f0` | Not connected |
| `0x1b` | `0x90170110` | Fixed, internal, Speaker, Other Analog, assoc 1 seq 0. **The speaker is on 0x1b, not on 0x14/0x17** |
| `0x1d` | `0x4063002d` | Realtek customisation value (checksum valid, low byte `0x2d`) |
| `0x1e`, `0x1f`, `0x21` | `0x411111f0` | Not connected. **There is no headphone pin (0x21) and no headset microphone (0x19)** |

Besides the pins, the registry holds saved coefficients for `0x53`/`0x54`. Linux writes no codec coefficients of its own (2.4, G3).

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

Voice Clarity (`voiceclaritycpuapo.dll`) is installed but is not in the chains above.

What these stages do inside is not reproduced here. The chain this repository uses on Linux is built from measurements of this device
only; it is in G4 and in the [platform files](../../../userspace/platform/README.md) (speaker processing).

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
- Result: **no fixup**. The generic parser builds the paths from the pin defaults the BIOS left, and no codec coefficients are written (1.2).
- The only audio patch in linux-surface `patches/7.3` is `0002-surface3.patch` (Surface 3 RT5645).

## 3. Gaps

In the "On Linux" column, "Check" is the command or procedure and "Measured" is what it gave on this device.

| # | Gap | Evidence | Impact | How to close it | Confidence | On Linux |
|---|---|---|---|---|---|---|
| G1 | The DSP path cannot be determined from the Windows side. On SOF, the sibling SL5 hangs on the second s2idle entry (1f.3 stays in D0) | 2.1. #1010 | Audio may play while suspend stalls | If the device comes up with SOF and the suspend problem reproduces, `snd_intel_dspcfg.dsp_driver=1` (legacy). The NHLT has no DMIC, so no function is lost (BT and USB offload and WoV do not exist on Linux anyway) | Path: inferred. Nothing lost on legacy: confirmed (NHLT) | Check: `dmesg \| grep -iE 'dspcfg\|SoundWire enabled\|Digital mics\|sof-audio'`, `lspci -k -s 00:1f.3`, `rtcwake -m freeze -s 20` twice. Measured: legacy `snd_hda_intel` (2.1). This problem occurs only on SOF and does not apply to this device. When idle, `00:1f.3` is `runtime_status: suspended`, `D3hot`. After playback, 3 consecutive runs of `rtcwake -m freeze -s 20` enter and resume 3/3 and playback is normal afterwards (PWR-3) |
| G2 | No quirk for codec SSID `10ec:1284`. The pin configuration depends on the BIOS defaults | 2.4, 1.2 | If the BIOS values differ from the table in 1.2, a headphone/headset port that does not exist shows up (0x19/0x21) or the speaker and DMIC are detected wrongly | If there is a difference, put `[codec] 0x10ec0274 0x10ec1284 0` + `[pincfg]` (the table in 1.2) in `/lib/firmware/sp9-alc274.fw` and set `options snd-hda-intel patch=sp9-alc274.fw` (legacy path). On the kernel side, `HDA_CODEC_QUIRK(0x10ec, 0x1284, "Microsoft Surface Pro 9", ALC274_FIXUP_SURFACE_PRO_9)` (matching on the codec SSID is required: the PCI SSID `8086:7270` is a generic Intel value) + a `HDA_FIXUP_PINS` table | Measured | Check: `cat /sys/class/sound/hwC*D0/init_pin_configs`. `dmesg \| grep -i 'ALC274\|autoconfig\|picked fixup'`. Measured: the 14 `init_pin_configs` are the table in 1.2 (0x12 `b7a60120`, 0x13 `40000000`, 0x1b `90170110`, 0x1d `4063002d`, the rest `411111f0`). `driver_pin_configs` and `user_pin_configs` are empty and there is no fixup. autoconfig gives `line_outs=1 (0x1b) type:speaker`, `Mic=0x12`. No pin patch is needed |
| G3 | Linux does not write the codec coefficients the registry holds for `0x53`/`0x54` (1.2) | 1.2, 2.4 | Unknown. Left/right level or tone may differ from Windows if these nodes matter | Read what the codec holds while Windows runs it, then write those values as `SET_COEF_INDEX`/`SET_PROC_COEF` pairs under `[verb]` in the patch file (in the kernel, a `struct coef_fw` array in `HDA_FIXUP_ACT_INIT`), and A/B compare | Effect unverified | Check: `hda-verb /dev/snd/hwC0D0 0x53 SET_COEF_INDEX 0x1e`, then `hda-verb ... 0x53 GET_PROC_COEF 0`, and record the current values; the same for 0x54. Not done: reading a coefficient needs a `SET_COEF_INDEX` write first |
| G4 | No speaker protection processing. Windows always runs software processing on the speakers (1.5). With no smart amplifier, software is all the protection there is | 1.5, the amplifier row of 1.1 | Measured on the raw sink (built-in microphone, tones at −12 dBFS, volume 100 %): below about 60 Hz the speakers return a fundamental 41–55 dB under the 400 Hz level, with THD 8–60 %. Each speaker alone (−6 dBFS, 1/24 octave) has a rise between 3.4 and 4.2 kHz that peaks 7–8 dB above its level at 2.7 kHz (left at 3.6 kHz, right at 4.0 kHz), and THD there is 5–9 % (left) and 3.5–6 % (right) against 1–2 % at 2.7 kHz. At full scale the 3.7 kHz THD was 16.9 % and the 100 Hz THD 72 % (AUD-2) | A speaker-only sink with PipeWire filter-chain. Each stage and the measurement it rests on: 2 stages of `bq_highpass` 50 Hz Q 0.707 (4th-order, −6 dB at 50 Hz; the speakers reproduce almost nothing below 60 Hz), `bq_peaking` 3700 Hz −6 dB Q 2.5 (the 3.4–4.2 kHz rise and its distortion; the centre lies between the left peak at 3.6 kHz and the right at 4.0 kHz, the depth stays under the 7–8 dB rise), output limiter −1 dBFS (a 1 dB margin for inter-sample peaks) | Measured (built-in microphone, which adds its own response). How it sounds: unverified | Check: tones through the filter sink and the raw sink, recorded with the built-in microphone, and the raw sink's monitor for the digital transfer. Measured (this configuration, loaded as a separate filter-chain instance): digital transfer 30 Hz −18.8, 40 Hz −10.7, 50 Hz −5.9, 100 Hz −0.5, 1 kHz −0.1, 3.6–3.8 kHz −5.9, 5 kHz −1.5, 10 kHz −0.1 dB. Acoustic, each speaker alone at −6 dBFS: 3.6 kHz left −15.0 → −20.4 dBFS; THD between 3.2 and 4.2 kHz left 4.7–8.9 → 2.3–4.3 %, right 1.8–6.2 → 0.8–3.2 %. Both speakers at −12 dBFS: 30–50 Hz fundamental 3–24 dB lower (median 11 dB), close to the noise floor. **The 100 Hz–1 kHz range is not held down**: 100 Hz at full scale had 72 % THD on the raw sink (AUD-2) and this chain does not change that band. The raw sink must be at 100 %: left at WirePlumber's default of 40 %, the amplifier sits at −23.9 dB even with the filter sink at 100 % (measured). The platform files raise the raw sink to 100 % every time WirePlumber starts ([platform files](../../../userspace/platform/README.md), the section on speaker processing). How it sounds needs a new listening check with this chain (AUD-1, AUD-2) |
| G5 | The microphones deliver only the raw 2-channel DMIC signal. Windows applies the Surface Capture APO (AEC/beamforming) and the Realtek recording EFX | 1.3, 1.5 | Echo and noise in calls. On the SLS2 (same codec, same DMIC on 0x12) stereo phase cancellation and Mic Boost clipping are reported (#2203) | Check the phase first. If one side is inverted: `options snd-hda-intel model=inv-dmic` (legacy, card 0), or `[model]` in the patch file, or an `ALC269_FIXUP_INV_DMIC` chain. Default Mic Boost 0 dB. Create a virtual source with PipeWire `module-echo-cancel` (webrtc, AEC+NS) and downmix to mono. If beamforming geometry is needed, use the coordinates in 1.3 (y=±21 mm) | No processing: confirmed. Phase inversion: no (same polarity in a Windows recording of ambient noise, sum−difference +9–11 dB. That value went through the APO path; the raw signal on Linux is in the next column) | Check: `arecord -D hw:0,0 -c2 -r48000 -f S16_LE -d 5 m.wav`, compare the RMS of `sox m.wav -n remix 1,2 stat` and `remix 1,2v-1 stat` (inverted if the difference signal is larger than the sum when speaking from the front), `amixer -c0 contents \| grep -i boost`. Measured: signal on both channels, level difference 0.6~0.8 dB. The polarity is the same (with both speakers playing at once, sum−difference +10.9 dB, correlation +0.88; by the sox method, sum RMS 0.0336 > difference 0.0235). `model=inv-dmic` is not needed. It was not measured with speech from the front. The default state is `Capture` +30 dB + `Mic Boost` +30 dB (source volume 100 %), and it clips there (155 samples on ch0 and 96 on ch1 in 2 seconds of a 1 kHz tone). At `Mic Boost` steps 0–2 the count is 0. When capture is opened as the codec wakes from runtime suspend, the first 1.0 second is stuck at full scale. The cause of this transient is unverified (AUD-3). The platform files contain a WirePlumber rule that sets the default gain to +30 dB (`Capture` +30 dB, `Mic Boost` 0 dB); it applies only to accounts with no stored value. They also contain an echo-cancel source `sp9_echo_cancel`: the source appears and all its nodes are suspended when it is not in use (measured). With the speakers playing synthetic speech it removes the echo to its noise floor, about 60 dB below the raw echo (measured, AUD-4). Speech from the near end during playback and a real call are unverified |
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
3. Coefficients (G3): read the indices of the processing nodes of 1.2 with `hda-verb` (from `alsa-tools`) and record them.
   Result: not done (G3).
4. Speakers: left/right position with `speaker-test -c2 -t wav`, the EAPD state (`EAPD` of Node 0x1b in `codec#0`), distortion over volume steps 0–100%. If there is no sound, look at the 0x1b EAPD and the G3 coefficients first.
   Result (measured): 0x1b `EAPD 0x2`, `Pin-ctls 0x40 OUT`. The left and right channels each produce sound, are on opposite sides, and have the same polarity. However, playback L reaches capture ch1 first and louder.
   Whether the speakers or the microphone channels are swapped is unverified. Distortion over the volume steps with the current filter chain is to be measured again (AUD-1).
5. Compare the same material before and after the filter chain (G4) and see whether distortion below 60 Hz and around 3.7 kHz is reduced.
   Result (measured): see G4.
6. Microphones: 2-channel recording, level per channel, phase (the sox comparison of G5), clipping per Mic Boost step, a call application on the echo-cancel source.
   Result (measured): see G5.
7. Port display (G6): `wpctl status` must show no headphone or headset port and the internal microphone must be `available`.
   Result (measured): see G6.
8. Suspend (G1): `rtcwake -m freeze -s 20` at least 2 times in a row, with `cat /sys/bus/pci/devices/0000:00:1f.3/power/runtime_status` before each. If it hangs on SOF, the same test with `dsp_driver=1`.
   Result (measured): see G1 (PWR-3 pass).
9. BT audio (A2DP playback, HFP microphone), HDMI audio playback on a USB-C/DP monitor.
   Result: unverified (AUD-5, AUD-6). The A2DP and HFP endpoints are registered with BlueZ and there are 4 HDMI/DP PCMs. No Bluetooth audio device has been paired, and audio over an external monitor has not been tried.
10. Keeping the basis of the path decision: the master-count and quirk-mask of `SNDW` are runtime NVS values, so record the SoundWire lines of `dmesg` and whether an SNDW device exists under `/sys/bus/acpi/devices/`.
    Result (measured): ACPI `\_SB_.PC00.HDAS.IDA_.SNDW` exists (status 15). The `SWMC` and `SWQ0-3` values were not read (2.1).
