# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# Windows baseline test tool. Collects logs automatically while a person runs the docs/test-plan.md IDs on Windows.
# The prompts and the result notes it writes are in Korean.
#   baseline.ps1 -List                     IDs with a one-line description
#   baseline.ps1 -Test PEN-1,PEN-2         interactive run
#   baseline.ps1 -Area PEN                 a whole area
#   baseline.ps1 -Test DSP-2 -DryRun       no person: waits -StepSeconds at each step and goes on (records no result; steps that make sound only with -AllowSound)
#   baseline.ps1 -Auto [-AllowSound]       measures only what needs no person and records the result (asks nothing). Scope is $AutoPlan; narrow it with -Test/-Area
# Raw data goes to private/windows/baseline/<ID>/<time>/, the summary to hw/windows/baseline/<ID>.md, the result line to hw/windows/baseline/results.md.
# Changes no system setting. Exception: during audio tests it sets the speaker endpoint volume and restores it afterwards.
param(
    [switch]$List,
    [string[]]$Test,
    [string]$Area,
    [switch]$DryRun,
    [int]$StepSeconds = 3,
    [switch]$AllowSound,
    [switch]$NoElevated,
    [switch]$Auto
)
$ErrorActionPreference = 'Continue'
. (Join-Path $PSScriptRoot 'common.ps1')
[Console]::OutputEncoding = New-Object Text.UTF8Encoding $false
$OutputEncoding = [Console]::OutputEncoding
$Utf8 = New-Object Text.UTF8Encoding $false
$Inv = [Globalization.CultureInfo]::InvariantCulture

# ---------------------------------------------------------------- test definitions
# Caps: pointer | events | hid | hid-elev | bt | battery | perf | sensors | brightness | presence | power-mode | winevents
# Info (snapshots at start and end): display | pointerdevs | hidcaps-ble | camlist | audiodev | btdevs | wlan | sam-events | penbattery
# Steps: a string = the person does it and presses Enter. @{Say=...; Run={...}} = runs automatically, then the next step.
# Na: no corresponding test on Windows (with the reason). Manual: collects nothing, only asks for the result.
function S([string]$say, [scriptblock]$run) { @{ Say = $say; Run = $run } }

$Tests = @(
    # ---- PEN
    @{ Id = 'PEN-1'; Name = '압력 범위와 획 시작 압력'; Needs = '펜'; Caps = 'pointer'; Info = 'pointerdevs'
        Do = '캔버스에 가볍게 대기→세게 누르기 10회, 가장 약한 획 10회. 원시 압력(0..4096/0..65535)과 획 시작 압력'
        Steps = @('펜으로 캔버스에 점을 찍듯 가볍게 댔다가 끝까지 세게 누르고 떼기를 10회 한다.', '[지우기] 가장 약한 힘으로 선 긋기를 10회 한다(선이 시작되는 최소 힘).') }
    @{ Id = 'PEN-2'; Name = '기울기'; Needs = '펜, 각도기'; Caps = 'pointer'; Info = 'pointerdevs'
        Do = '각도기로 30/45/60° 네 방향 고정. 단계별 tiltX/Y 평균·편차'
        Steps = @(foreach ($a in 30, 45, 60) { foreach ($d in '오른쪽(+X)', '왼쪽(-X)', '화면 아래쪽(+Y, 사용자 쪽)', '화면 위쪽(-Y)') { "펜촉을 화면에 댄 채 $d 으로 ${a}° 기울여 3초 고정한 뒤 Enter." } }) }
    @{ Id = 'PEN-3'; Name = '옆 버튼과 오검출'; Needs = '펜'; Caps = 'pointer'
        Do = '옆 버튼 10회, 이어서 버튼 없이 30초 드로잉. 눌림 횟수와 드로잉 중 오검출'
        Steps = @('펜촉을 화면 가까이(호버) 둔 채 옆 버튼을 10회 눌렀다 뗀다.', '[지우기] 버튼을 건드리지 않고 30초 동안 계속 그린다.') }
    @{ Id = 'PEN-4'; Name = '지우개'; Needs = '펜'; Caps = 'pointer'
        Do = '지우개 끝으로 접촉. Eraser/Invert 플래그'
        Steps = @('펜을 뒤집어 지우개 끝으로 캔버스를 5번 문지른다(빨간 선이 나오면 지우개로 인식).') }
    @{ Id = 'PEN-5'; Name = 'BLE 연결·배터리'; Needs = '펜(잠든 상태에서 시작)'; Caps = 'bt', 'presence', 'events'; Info = 'penbattery', 'btdevs'; Elevated = $true
        Do = '잠든 펜을 꼬리 버튼으로 깨워 다시 연결되는 시간, 펜 배터리 %, BLE ETW'
        Steps = @('펜을 몇 분 쓰지 않아 잠들게 둔다(장치 노드가 사라지면 잠든 것). 준비되면 Enter.', '꼬리 버튼을 한 번 눌러 펜을 깨운다. 펜으로 화면을 눌러 반응하면 바로 Enter(이 단계 시간 = 재연결 시간).', '설정 > 블루투스 및 장치에서 펜 배터리 %를 확인하고 Enter.') }
    @{ Id = 'PEN-6'; Name = '꼬리 버튼'; Needs = '펜(깨어 있음)'; Caps = 'events', 'hid', 'presence'; Kbd = $true
        Do = '꼬리 버튼 1회·2회·길게. BLE 키보드 컬렉션으로 오는 키 조합'
        Steps = @('꼬리 버튼을 한 번 누른다(열리는 앱은 닫는다).', '꼬리 버튼을 두 번 누른다.', '꼬리 버튼을 길게 누른다.') }
    @{ Id = 'PEN-7'; Name = '디지타이저 경유 펜 리포트(Col07)'; Needs = '펜'; Caps = 'pointer', 'hid-elev'; Elevated = $true
        Do = 'THC Col07(FFF4) 입력 리포트 84/110 수신 여부(승격 hidapi, 주소 바이트는 private만)'
        Steps = @('펜을 화면에 대고 10초 동안 그린다.', '펜을 화면에서 멀리 떼고 5초 기다린다.') }
    @{ Id = 'PEN-8'; Name = '펌웨어 컬렉션 입력(중복 입력의 윈도우 대응)'; Needs = '펜'; Caps = 'pointer'
        Do = 'THC Col09(펜)·Col06(터치)와 HEAT 가상 장치 중 어느 쪽으로 Raw Input 리포트가 오는지'
        Steps = @('펜으로 10초 그린다.', '손가락으로 10초 그린다.') }
    @{ Id = 'PEN-9'; Name = '햅틱'; Needs = '펜(깨어 있음)'; Caps = 'bt', 'hid', 'pointer'; Info = 'hidcaps-ble'; Elevated = $true
        Do = '햅틱이 나는 동안 BLE ETW와 펜 BLE HID 캡처(윈도우가 보내는 리포트 65 확인용)'
        Steps = @('설정 > 블루투스 및 장치 > 펜 및 Windows Ink에서 "촉각 신호"가 켜져 있는지만 확인한다(바꾸지 않는다). Enter.', '햅틱을 내는 앱(Whiteboard, OneNote 등)에서 20초 그리고 지운다. 진동을 느꼈는지 기억해 둔다.', '펜을 내려놓고 Enter.') }
    # ---- TOUCH
    @{ Id = 'TOUCH-1'; Name = '약한 탭·스와이프 누락'; Needs = ''; Caps = 'pointer'; Info = 'pointerdevs'
        Do = '손끝 약한 탭 20회, 세 손가락 스와이프 10회. 인식 횟수'
        Steps = @('손끝으로 캔버스를 가볍게 20회 탭한다.', '[지우기] 캔버스 안에서 세 손가락 스와이프를 10회 한다(시스템 제스처가 가져가면 그 사실을 메모).') }
    @{ Id = 'TOUCH-2'; Name = '손바닥 차단'; Needs = '펜'; Caps = 'pointer'
        Do = '펜 호버 중 손바닥 접촉. 펜 근접 중 들어온 터치 수'
        Steps = @('펜을 화면 위에 띄운(호버) 채 손바닥을 화면에 5번 댄다.', '[지우기] 펜 없이 손바닥을 5번 댄다(비교용).') }
    @{ Id = 'TOUCH-3'; Name = '대기 후 복구'; Needs = '펜'; Caps = 'pointer', 'winevents', 'events'
        Do = '모던 스탠바이 진입·복귀 10회, 매번 터치·펜. Kernel-Power 506/507'
        Steps = @('전원 버튼으로 대기 진입 → 다시 전원 버튼으로 복귀를 10회. 매번 캔버스에 손가락과 펜으로 선을 긋는다. 끝나면 Enter.') ; Timeout = 1800 }
    @{ Id = 'TOUCH-4'; Name = '터치로 깨우기'; Needs = '펜'; Caps = 'events', 'winevents'
        Do = '화면 꺼짐·대기 중 터치·펜으로 깨어나는지'
        Steps = @('전원 버튼으로 대기에 들어간 뒤 화면을 두드리고 펜을 대 본다. 깨어나지 않으면 전원 버튼으로 깨운다. Enter.') }
    # ---- CAM
    @{ Id = 'CAM-1'; Name = '후면 캡처와 방향'; Needs = ''; Caps = @(); Info = 'camlist'
        Do = '후면 1920x1080·3840x2160 각 3초 캡처, 실측 fps, 마지막 프레임 저장 후 방향 확인'
        Steps = @((S '후면 카메라 캡처 중' { Invoke-Cam Rear 1920x1080 30 3; Invoke-Cam Rear 3840x2160 30 3 -Show }), '열린 사진이 바로 서 있는지 본다. Enter.') }
    @{ Id = 'CAM-2'; Name = '전면 캡처'; Needs = ''; Caps = @(); Info = 'camlist'
        Do = '전면 1920x1080·1920x1440 캡처, 실측 fps, IR 카메라 모드(WinRT)'
        Steps = @((S '전면 카메라 캡처 중' { Invoke-Cam Front 1920x1080 30 3; Invoke-Cam Front 1920x1440 30 3 -Show }), '열린 사진이 바로 서 있는지 본다. Enter.') }
    @{ Id = 'CAM-3'; Name = '모드별 스트리밍'; Needs = ''; Caps = @(); Info = 'camlist'
        Do = '전면·후면이 내놓는 모든 해상도에서 2초씩 캡처(윈도우는 전면 센서를 1296x972 비닝 한 모드로 돌리고 ISP가 키운다)'
        Steps = @((S '모든 모드를 차례로 캡처 중(1–2분)' { Invoke-CamAll })) }
    @{ Id = 'CAM-4'; Name = '자동초점'; Needs = '가까운 물체(10 cm), 먼 배경'; Caps = @()
        Do = '후면으로 가까이·멀리 찍고 선명도(라플라시안 분산) 비교'
        Steps = @('후면 카메라 앞 10 cm에 글씨가 있는 물체를 둔다. Enter.', (S '가까이 캡처' { Invoke-Cam Rear 1920x1080 30 4 -Tag near -Sharp }), '물체를 치우고 2 m 이상 떨어진 배경을 향한다. Enter.', (S '멀리 캡처' { Invoke-Cam Rear 1920x1080 30 4 -Tag far -Sharp })) }
    @{ Id = 'CAM-5'; Name = '프라이버시 LED'; Needs = ''; Caps = @()
        Do = '전면·후면 각 10초 스트리밍 중 어느 LED가 켜지는지 관찰'
        Steps = @((S '전면 10초 스트리밍: 기기의 LED를 본다' { Invoke-Cam Front 1280x720 30 10 }), (S '후면 10초 스트리밍: LED를 본다' { Invoke-Cam Rear 1280x720 30 10 }), '본 것을 메모할 준비가 되면 Enter.') }
    @{ Id = 'CAM-6'; Name = '영상 품질'; Needs = '같은 실내 조명'; Caps = @()
        Do = '전면·후면 최대 해상도 프레임을 private/에 저장(리눅스 사진과 나란히 비교)'
        Steps = @((S '최대 해상도 프레임 저장' { Invoke-Cam Front 1920x1440 30 3 -Tag quality; Invoke-Cam Rear 3840x2160 30 3 -Tag quality }), '원하면 윈도우 카메라 앱 사진도 같은 폴더에 넣는다. Enter.') }
    # ---- SAM
    @{ Id = 'SAM-1'; Name = '타입 커버 탈착'; Needs = '타입 커버'; Caps = 'events', 'presence', 'winevents'
        Do = '커버 떼고 붙이기 5회. 장치 노드·태블릿 모드 변화'
        Steps = @('커버를 떼고 붙이기를 5회. 매번 키 입력·터치패드·백라이트 키를 확인한다. Enter.') }
    @{ Id = 'SAM-2'; Name = '태블릿 모드'; Needs = '타입 커버'; Caps = 'events', 'presence'
        Do = '커버 뒤로 접기·떼기. ConvertibleSlateMode 변화'
        Steps = @('커버를 뒤로 접는다. Enter.', '커버를 다시 펴서 쓰는 자세로. Enter.', '커버를 뗀다. Enter.', '커버를 붙인다. Enter.') }
    @{ Id = 'SAM-3'; Name = '버튼'; Needs = ''; Caps = 'events', 'winevents'
        Do = '볼륨 위/아래 각 5회, 전원 버튼 대기·복귀. Raw Input 소비자/시스템 리포트'
        Steps = @('볼륨 위 버튼 5회.', '볼륨 아래 버튼 5회.', '전원 버튼으로 대기에 들어갔다가 전원 버튼으로 깨운다. Enter.') }
    @{ Id = 'SAM-4'; Name = '부하 후 팬(리눅스 재부팅 준비)'; Needs = '충전기'; Caps = 'perf', 'winevents'
        Do = '윈도우에서 5분 전 코어 부하. 끝나면 바로 리눅스로 재부팅해 팬 RPM 확인'
        Steps = @((S '5분 전 코어 부하 중' { Invoke-Stress 300 }), '이제 리눅스로 재부팅해 유휴 5분 뒤 sensors를 본다(리눅스 SAM-4). Enter.') }
    @{ Id = 'SAM-5'; Name = '전원 모드별 성능'; Needs = '충전기'; Caps = 'perf', 'power-mode', 'winevents'
        Do = '윈도우 전원 모드 3단계 각각 120초 부하. 유효 클럭·패키지 W·온도'
        Steps = @('설정 > 시스템 > 전원에서 전원 모드를 "최고 전력 효율"로 둔다. Enter.', (S '120초 부하' { Invoke-Stress 120 }), '전원 모드를 "균형"으로. Enter.', (S '120초 부하' { Invoke-Stress 120 }), '전원 모드를 "최고 성능"으로. Enter.', (S '120초 부하' { Invoke-Stress 120 }), '전원 모드를 원래대로 돌려 놓는다. Enter.') }
    @{ Id = 'SAM-6'; Name = '배터리·충전'; Needs = '충전기'; Caps = 'battery', 'events', 'winevents'
        Do = 'AC 탈착 5회. 상태 갱신과 충전율'
        Steps = @('충전기를 뽑고 꽂기를 5회(각 10초 이상). Enter.') }
    @{ Id = 'SAM-7'; Name = '회전·자세 센서'; Needs = ''; Caps = 'sensors', 'events'
        Do = '네 방향 + 눕히기. 가속도 벡터(리눅스 mount matrix 기준)'
        Steps = @('가로, 카메라가 위로 세운다. 3초 뒤 Enter.', '시계 방향으로 90° 돌려 세운다. 3초 뒤 Enter.', '180°(카메라가 아래). 3초 뒤 Enter.', '반시계 방향 90°. 3초 뒤 Enter.', '화면이 위를 보게 책상에 눕힌다. 3초 뒤 Enter.') }
    @{ Id = 'SAM-8'; Name = 'SAM 이벤트 관찰'; Needs = '충전기, 타입 커버'; Caps = 'winevents', 'events', 'perf'; Info = 'sam-events'
        Do = '커버·충전기·부하 변화 중 Surface 이벤트 로그와 SMF 전력 제한 갱신'
        Steps = @('커버를 떼고 붙인다. Enter.', '충전기를 뽑고 꽂는다. Enter.', (S '30초 부하' { Invoke-Stress 30 })) }
    # ---- AUD
    @{ Id = 'AUD-1'; Name = '스피커 좌우·찌그러짐'; Needs = '조용한 방'; Caps = @(); Sound = $true
        Do = '볼륨 25/50/75%(100%는 확인 후)에서 1 kHz 좌·우·양쪽, 50%에서 스윕. 마이크로 좌우 균형·THD'
        Steps = @((S '볼륨 단계별 1 kHz 톤' { foreach ($v in 25, 50, 75) { Invoke-Audio tones $v '--freqs 1000' "vol$v" } ; if (Confirm-Ask '100% 볼륨으로도 짧게 재생할까?') { Invoke-Audio tones 100 '--freqs 1000 --level -24' 'vol100' } }),
            (S '50%에서 스윕(좌, 우)' { Invoke-Audio sweep 50 }), '좌우가 맞게 들렸는지, 찌그러짐이 있었는지 메모할 준비가 되면 Enter.') }
    @{ Id = 'AUD-2'; Name = '스피커 처리(APO 켬/우회)'; Needs = '조용한 방'; Caps = @(); Sound = $true; Setting = 'APO 켬·우회 둘 다'
        Do = '40/100/3700 Hz 톤과 스윕을 APO 켬(공유)과 우회(배타 재생)로. 리눅스 필터 체인 끔/켬과 비교'
        Steps = @((S 'APO 켬(공유 모드) 톤·스윕' { Invoke-Audio tones 75 '--freqs 40,100,3700' 'vol75'; Invoke-Audio sweep 50 }),
            (S 'APO 우회(배타 모드) 톤·스윕' { Invoke-Audio tones 75 '--freqs 40,100,3700 --mode exclusive' 'vol75'; Invoke-Audio sweep 50 '--mode exclusive' }), '두 경우의 소리 차이를 메모할 준비가 되면 Enter.') }
    @{ Id = 'AUD-3'; Name = '내장 마이크'; Needs = '조용한 방'; Caps = @(); Sound = $true
        Do = '스피커 스윕으로 마이크 간 지연·극성, 정면 발화 5초로 채널 레벨·합−차·클리핑'
        Steps = @((S '스윕으로 마이크 위상 측정' { Invoke-Audio sweep 50 }), '화면 정면 30 cm에서 Enter 직후 5초 동안 평소 크기로 말한다.', (S '녹음 5초' { Invoke-Audio record 0 '--seconds 5' })) }
    @{ Id = 'AUD-4'; Name = '통화 에코'; Needs = '상대방'; Manual = $true
        Do = '화상 통화 앱으로 스피커 재생 중 통화. 상대방이 에코를 듣는지(관찰만)'; Steps = @('통화 앱으로 시험한다. 끝나면 Enter.') }
    @{ Id = 'AUD-5'; Name = '블루투스 오디오'; Needs = '블루투스 이어폰'; Caps = 'events'; Info = 'audiodev', 'btdevs'
        Do = 'A2DP 재생·HFP 통화 마이크. 엔드포인트 목록 전후'
        Steps = @('블루투스 이어폰을 연결해 음악을 재생한다. Enter.', '통화 앱에서 이어폰 마이크로 말해 본다. Enter.') }
    @{ Id = 'AUD-6'; Name = '외부 모니터 오디오'; Needs = 'USB-C DP/HDMI 모니터(스피커)'; Caps = 'events'; Info = 'audiodev', 'display'
        Do = '외부 모니터 스피커 재생. 엔드포인트 목록 전후'
        Steps = @('모니터를 연결하고 그 스피커로 소리를 낸다. Enter.') }
    # ---- DSP
    @{ Id = 'DSP-1'; Name = '화면 깜빡임'; Needs = '충전기'; Caps = 'events', 'brightness', 'battery', 'perf'; Timeout = 1800
        Do = '정지 화면·타이핑·스크롤 각 수 분, AC와 배터리. 깜빡임 관찰(윈도우 PSR은 직접 못 봄), 패키지 W'
        Steps = @('배터리 상태에서 정지 화면 3분. Enter.', '배터리 상태에서 타이핑·스크롤 3분. Enter.', '충전기를 꽂고 정지 화면 2분, 타이핑·스크롤 2분. Enter.') }
    @{ Id = 'DSP-2'; Name = '60↔120 Hz'; Needs = ''; Caps = 'events'; Info = 'display'
        Do = '설정에서 60↔120 Hz 전환 5회. 모드 변화 시각과 화면 꺼짐 여부'
        Steps = @('설정 > 디스플레이 > 고급 디스플레이 > 재생 빈도를 60↔120 Hz로 5회 바꾼다(매번 화면이 꺼졌다 켜지는지 본다). 마지막엔 원래 값. Enter.', '"동적 재생 빈도" 항목이 있으면 그 상태만 메모한다. Enter.') }
    @{ Id = 'DSP-3'; Name = '백라이트 최저'; Needs = ''; Caps = 'brightness', 'events'
        Do = '밝기를 0까지 내렸다 올리기. 0에서 화면이 꺼지는지'
        Steps = @('밝기를 끝까지(0) 내리고 화면이 완전히 꺼지는지 본다. Enter.', '밝기를 원래대로 올린다. 조절 중 깜빡였는지 본다. Enter.') }
    @{ Id = 'DSP-4'; Name = '조도 센서'; Needs = '평소 실내 조명'; Caps = 'sensors'
        Do = '평소 조명과 센서 가리기. lux'
        Steps = @('평소 조명에서 기기를 세워 둔다. 5초 뒤 Enter.', '전면 카메라 옆 센서를 손가락으로 가린다. 5초 뒤 Enter.') }
    @{ Id = 'DSP-5'; Name = '외부 모니터·도크'; Needs = 'USB-C 모니터, 충전기'; Caps = 'events', 'presence', 'winevents'; Info = 'display'
        Do = 'USB-C 두 포트 각각 모니터·충전기 연결·분리'
        Steps = @('포트 1에 모니터를 연결하고 화면이 나오면 분리. Enter.', '포트 2에 모니터를 연결하고 화면이 나오면 분리. Enter.', '각 포트에 충전기를 꽂았다 뺀다. Enter.') }
    # ---- PWR
    @{ Id = 'PWR-1'; Name = '대기 왕복'; Needs = ''; Caps = 'events', 'winevents', 'battery'; Timeout = 1800
        Do = '전원 버튼으로 모던 스탠바이 진입·복귀 10회. Kernel-Power 506/507'
        Steps = @('전원 버튼으로 대기 진입, 10초 뒤 복귀를 10회. Enter.') }
    @{ Id = 'PWR-2'; Name = '덮개(커버)와 대기'; Needs = '충전기, 타입 커버'; Caps = 'events', 'winevents'
        Do = '충전 중 커버 닫기→열기 5회. 덮개 알림과 대기 진입·복귀'
        Steps = @('충전기를 꽂은 채 커버를 닫고 10초 뒤 연다. 5회. Enter.') }
    @{ Id = 'PWR-3'; Name = '오디오 재생 후 연속 대기'; Needs = ''; Caps = 'events', 'winevents'; Sound = $true
        Do = '짧게 소리 재생 후 대기 진입·복귀 3회'
        Steps = @((S '1초 톤 재생' { Invoke-Audio tones 30 '--freqs 1000 --seconds 1' 'pre-sleep' }), '전원 버튼으로 대기에 들어가 20초 뒤 깨우기를 3회. Enter.') }
    @{ Id = 'PWR-4'; Name = '대기 전력'; Needs = '1시간'; Caps = 'battery', 'winevents'; Timeout = 14400; Elevated = $true
        Do = '배터리로 1시간 대기. 잔량 변화로 mW, 끝나고 sleepstudy(HW DRIPS)'
        Steps = @('충전기를 뽑는다. Enter 후 바로 전원 버튼으로 대기에 들어간다.', '1시간쯤 뒤 깨우고 Enter.', (S 'sleepstudy 수집(승격)' { Invoke-Elevated sleepstudy (Join-Path $script:Raw 'power') })) }
    @{ Id = 'PWR-5'; Name = '런타임 화면 끔 PC10'; Na = '윈도우는 화면만 끈 런타임 상태를 따로 시험할 수 없다. 대기 중 HW DRIPS(PWR-4 sleepstudy)가 대응 기준값이다' }
    @{ Id = 'PWR-6'; Name = 'RTC 깨우기'; Na = '윈도우에서 하려면 깨우기 타이머를 설정해야 해서(설정 변경) 하지 않는다' }
    @{ Id = 'PWR-7'; Name = '종료'; Needs = 'USB 허브(LED)'; Manual = $true
        Do = '윈도우 종료 5회, 허브 LED·전원 LED 관찰(관찰만, 도구는 종료 전후로 못 돈다)'; Steps = @('종료·켜기를 5회 하고 관찰한 뒤 이 도구를 다시 열어 결과만 기록한다. Enter.') }
    @{ Id = 'PWR-8'; Name = '최대절전'; Na = '이 윈도우는 최대절전을 껐다. 켜는 것은 설정 변경이다' }
    @{ Id = 'PWR-9'; Name = '열·지속 성능'; Needs = '충전기'; Caps = 'perf', 'battery', 'winevents'
        Do = '10분 전 코어 부하. 30초 구간별 유효 클럭·패키지 W·온도, SMF PL 갱신'
        Steps = @((S '10분 전 코어 부하 중(뒷면 온도를 손으로 느껴 둔다)' { Invoke-Stress 600 }), '뒷면 온도 느낌을 메모할 준비가 되면 Enter.') }
    # ---- NET
    @{ Id = 'NET-1'; Name = 'Wi-Fi'; Needs = '5 GHz AP, iperf3 서버(선택)'; Caps = 'events', 'winevents'; Info = 'wlan'
        Do = '연결 상태(대역·채널·속도·신호), iperf3 처리량(서버가 있으면), 대기 복귀 후 재연결'
        Steps = @((S 'iperf3' { Invoke-Iperf }), '전원 버튼으로 대기 진입·복귀 후 자동으로 다시 연결되는지 본다. Enter.') }
    @{ Id = 'NET-2'; Name = '블루투스'; Needs = '마우스·이어폰'; Caps = 'events', 'presence', 'winevents'; Info = 'btdevs'
        Do = '마우스·이어폰 연결, 대기 복귀 후 재연결'
        Steps = @('마우스와 이어폰을 연결해 쓴다. Enter.', '대기 진입·복귀 후 다시 붙는지 본다. Enter.') }
)

# ---------------------------------------------------------------- automatic mode (-Auto)
# Only the parts measurable without a person or external equipment. A test with no definition or with Skip is passed over as skipped.
# Every Step is S (automatic). Without Caps/Info the test definition's are used. Require: ac | battery | bt-audio | ext-monitor (Get-AutoEnv).
# Human: the part left in the result line as needing a person's check. Settings (brightness, refresh rate, power mode) are read, never changed.
# Never suspends, locks or powers off the device and never drops a connection. Sound only at volume 20%, -24 dBFS (-AllowSound).
$AutoAudio = '--level -24'
$AutoPlan = @{
    'CAM-1' = @{ Steps = @((S '후면 1920x1080·3840x2160 각 3초 캡처' { Invoke-Cam Rear 1920x1080 30 3; Invoke-Cam Rear 3840x2160 30 3 }))
        Human = '저장한 후면 프레임(private/…/cam/*.jpg)이 바로 서 있는지 본다' }
    'CAM-2' = @{ Steps = @((S '전면 1920x1080·1920x1440 각 3초 캡처' { Invoke-Cam Front 1920x1080 30 3; Invoke-Cam Front 1920x1440 30 3 }))
        Human = '저장한 전면 프레임이 바로 서 있는지 본다' }
    'CAM-3' = @{ Steps = @((S '전면·후면 모든 모드 2초씩 캡처' { Invoke-CamAll })) }
    'CAM-4' = @{ Skip = '가까운 물체와 먼 배경을 사람이 놓아야 한다' }
    'CAM-5' = @{ Skip = 'LED는 사람이 봐야 한다' }
    'CAM-6' = @{ Steps = @((S '전면·후면 최대 해상도 프레임 저장' { Invoke-Cam Front 1920x1440 30 3 -Tag quality; Invoke-Cam Rear 3840x2160 30 3 -Tag quality }))
        Human = '같은 조명에서 리눅스 사진과 나란히 비교한다' }
    'SAM-1' = @{ Skip = '타입 커버를 떼고 붙여야 한다' }
    'SAM-2' = @{ Skip = '커버를 접고 떼야 한다' }
    'SAM-3' = @{ Skip = '버튼을 눌러야 한다' }
    'SAM-4' = @{ Skip = '부하 직후 리눅스로 재부팅해야 의미가 있다(윈도우 쪽 부하 값은 PWR-9 자동 측정이 대신한다)' }
    'SAM-5' = @{ Require = 'ac'; Caps = @('perf', 'power-mode', 'winevents')
        Steps = @((S '유휴 30초' { Wait-Auto 30 }), (S '현재 전원 모드로 120초 전 코어 부하' { Invoke-Stress 120 }), (S '식힘 30초' { Wait-Auto 30 }))
        Human = '전원 모드 "최고 전력 효율"·"최고 성능"에서 같은 120초 부하(도구는 전원 모드를 바꾸지 않는다)' }
    'SAM-6' = @{ Caps = @('battery', 'events'); Steps = @((S '배터리 상태 60초' { Wait-Auto 60 }))
        Human = '충전기 탈착 5회와 표시 갱신, 80% 부근 충전 제한 상태' }
    'SAM-7' = @{ Caps = @('sensors'); Steps = @((S '현재 자세 10초' { Wait-Auto 10 }))
        Human = '네 방향 회전과 눕히기(다섯 자세의 가속도 벡터)' }
    'SAM-8' = @{ Caps = @('winevents', 'events', 'perf')
        Steps = @((S '유휴 20초' { Wait-Auto 20 }), (S '30초 부하' { Invoke-Stress 30 }), (S '식힘 20초' { Wait-Auto 20 }))
        Human = '커버 탈착·충전기 탈착 중 들어오는 이벤트' }
    'AUD-1' = @{ Steps = @((S '볼륨 20% 1 kHz 톤(-24 dBFS)' { Invoke-Audio tones 20 "--freqs 1000 $AutoAudio" 'vol20' }), (S '볼륨 20% 스윕(좌, 우)' { Invoke-Audio sweep 20 $AutoAudio }))
        Human = '귀로 좌우·찌그러짐 확인, 볼륨 25/50/75/100% 단계(큰 소리는 자동으로 내지 않는다)' }
    'AUD-2' = @{ Setting = 'APO 켬·우회 둘 다'
        Steps = @((S 'APO 켬(공유) 40/100/3700 Hz 톤·스윕' { Invoke-Audio tones 20 "--freqs 40,100,3700 $AutoAudio" 'vol20'; Invoke-Audio sweep 20 $AutoAudio }),
            (S 'APO 우회(배타) 톤·스윕' { Invoke-Audio tones 20 "--freqs 40,100,3700 $AutoAudio --mode exclusive" 'vol20'; Invoke-Audio sweep 20 "$AutoAudio --mode exclusive" }))
        Human = '75% 이상에서 두 경로의 소리 차이를 귀로(낮은 볼륨에서는 차이가 작다)' }
    'AUD-3' = @{ Steps = @((S '볼륨 20% 스윕으로 마이크 위상' { Invoke-Audio sweep 20 $AutoAudio }), (S '무음 5초 녹음(잡음 바닥)' { Invoke-Audio record 0 '--seconds 5' 'ambient' }))
        Human = '정면 30 cm에서 말하며 5초 녹음(합−차·클리핑)' }
    'AUD-5' = @{ Require = 'bt-audio'; Steps = @((S '엔드포인트 목록' { Wait-Auto 3 })); Human = '이어폰으로 재생·통화 마이크 소리' }
    'AUD-6' = @{ Require = 'ext-monitor'; Steps = @((S '엔드포인트 목록' { Wait-Auto 3 })); Human = '모니터 스피커로 소리' }
    'DSP-1' = @{ Caps = @('perf', 'brightness', 'battery', 'events'); Steps = @((S '정지 화면 60초(현재 전원)' { Wait-Auto 60 }))
        Human = '정지·타이핑·스크롤 중 깜빡임 관찰, 배터리 상태에서 같은 측정' }
    'DSP-2' = @{ Steps = @((S '현재 모드 5초 기록' { Wait-Auto 5 }))
        Human = '설정에서 60↔120 Hz 전환 5회(화면 꺼짐 관찰), 동적 재생 빈도 상태(도구는 주사율을 바꾸지 않는다)' }
    'DSP-3' = @{ Caps = @('brightness'); Info = @('brightness'); Steps = @((S '밝기 5초' { Wait-Auto 5 }))
        Human = '밝기를 0까지 내려 화면이 꺼지는지, 조절 중 깜빡임(도구는 밝기를 바꾸지 않는다)' }
    'DSP-4' = @{ Caps = @('sensors'); Steps = @((S '평소 조명 10초' { Wait-Auto 10 })); Human = '센서를 손가락으로 가려 lux가 내려가는지' }
    'DSP-5' = @{ Require = 'ext-monitor'; Steps = @((S '디스플레이 목록' { Wait-Auto 3 })); Human = 'USB-C 두 포트 각각 모니터·충전기 연결과 분리' }
    'PWR-1' = @{ Skip = '대기 진입·복귀가 필요하다(도구는 기기를 재우지 않는다)' }
    'PWR-2' = @{ Skip = '커버를 닫고 열어야 한다' }
    'PWR-3' = @{ Skip = '대기 진입·복귀가 필요하다' }
    'PWR-4' = @{ Caps = @('battery')
        Steps = @((S '지난 하루 sleepstudy(승격)' { Invoke-Elevated sleepstudy (Join-Path $script:Raw 'power') }),
            (S '배터리일 때만 유휴 5분 방전' { if ((Get-AutoEnv).Ac) { Say '  건너뜀: AC 연결 중이라 방전율을 재지 않는다' 'DarkYellow' } else { Wait-Auto 300 } }))
        Human = '충전기를 뽑고 1시간 대기 후 잔량 변화(mW)' }
    'PWR-9' = @{ Require = 'ac'; Caps = @('perf', 'battery', 'winevents')
        Steps = @((S '유휴 30초' { Wait-Auto 30 }), (S '전 코어 부하 300초' { Invoke-Stress 300 }), (S '식힘 60초' { Wait-Auto 60 }))
        Human = '10분 부하로 늘려 스로틀 진동 확인, 뒷면 온도 느낌' }
    'NET-1' = @{ Caps = @(); Steps = @((S 'Wi-Fi 상태 읽기' { Wait-Auto 2 }))
        Human = 'iperf3 처리량(서버 필요), 대기 복귀 후 재연결, 대역·채널·신호(위치 서비스를 켜야 보인다)' }
    'NET-2' = @{ Skip = '마우스·이어폰 연결과 대기 복귀가 필요하다' }
}

# ---------------------------------------------------------------- common functions
function Say([string]$s, [string]$color = 'Cyan') { Write-Host $s -ForegroundColor $color }
function Confirm-Ask([string]$q) {
    if ($DryRun) { return $false }
    $a = Read-Host "$q [y/N]"; return $a -match '^(y|yes|ㅇ|예)$'
}

function Wait-Next([int]$timeout) {
    if ($DryRun) { Start-Sleep -Seconds $StepSeconds; return 'dry' }
    if ([Console]::IsInputRedirected) { [void](Read-Host); return 'enter' }   # scripted input: one line per step
    $next = $script:NextFile
    if ($next) { Remove-Item $next -ErrorAction SilentlyContinue }
    $t0 = Get-Date
    while ($true) {
        if ([Console]::KeyAvailable) { $k = [Console]::ReadKey($true); if ($k.Key -eq 'Enter') { return 'enter' } }
        if ($next -and (Test-Path $next)) { Remove-Item $next -ErrorAction SilentlyContinue; return 'enter' }
        if ($timeout -gt 0 -and ((Get-Date) - $t0).TotalSeconds -gt $timeout) { return 'timeout' }
        Start-Sleep -Milliseconds 100
    }
}

function Wait-Auto([int]$sec) {
    if ($DryRun) { $sec = [math]::Min($sec, $StepSeconds) }
    Start-Sleep -Seconds $sec
}

# What is connected right now (read once per run): AC, active displays, Bluetooth audio sink, type cover.
function Get-AutoEnv {
    if ($script:AutoEnv) { return $script:AutoEnv }
    $b = Get-CimInstance -Namespace root\wmi -ClassName BatteryStatus -ErrorAction SilentlyContinue | Select-Object -First 1
    $disp = if (Test-Path $Kit.Probe) { @(& $Kit.Probe display | Where-Object { $_ -match '^display ' }).Count } else { 1 }
    $pnp = @(Get-PnpDevice -PresentOnly -ErrorAction SilentlyContinue | Where-Object { $_.Status -eq 'OK' })
    $script:AutoEnv = @{
        Ac       = [bool]$b.PowerOnline
        Displays = $disp
        BtAudio  = @($pnp | Where-Object { $_.InstanceId -match '^BTHENUM\\\{0000110B' }).Count -gt 0   # A2DP sink service
        Cover    = @($pnp | Where-Object { $_.InstanceId -match 'TARGET_KIP' }).Count -gt 0
    }
    return $script:AutoEnv
}

function Get-AutoSkip($t) {
    $p = $AutoPlan[$t.Id]
    if ($t.Na) { return "윈도우 대응 없음($($t.Na))" }
    if ($t.Manual) { return "사람이 관찰만 하는 시험" }
    if (-not $p) { return "사람이 펜·손으로 입력해야 한다" + $(if ($t.Needs) { " (필요: $($t.Needs))" }) }
    if ($p.Skip) { return $p.Skip }
    if ($t.Sound -and -not $AllowSound) { return '-AllowSound 없이는 소리 내는 시험을 하지 않는다' }
    $e = Get-AutoEnv
    switch ($p.Require) {
        'ac' { if (-not $e.Ac) { return '충전기가 연결되어 있지 않다(부하 측정은 AC에서 한다)' } }
        'battery' { if ($e.Ac) { return 'AC 연결 중이다' } }
        'bt-audio' { if (-not $e.BtAudio) { return '블루투스 오디오 장치가 연결되어 있지 않다' } }
        'ext-monitor' { if ($e.Displays -lt 2) { return '외부 모니터가 연결되어 있지 않다' } }
    }
    return $null
}

function Add-Marker([string]$step, [string]$text) {
    $f = Join-Path $script:Raw 'markers.csv'
    if (-not (Test-Path $f)) { [IO.File]::WriteAllText($f, "t,step,text`n", $Utf8) }
    [IO.File]::AppendAllText($f, ("{0},{1},{2}`n" -f (Get-UnixTime).ToString('F3', $Inv), $step, ($text -replace '[,\r\n]', ' ')), $Utf8)
}

function Start-Bg([string]$exe, [string[]]$argv) {
    Start-Process -FilePath $exe -ArgumentList $argv -WindowStyle Hidden -PassThru
}

function Invoke-Elevated([string]$action, [string]$dir, [string]$extra) {
    if ($NoElevated) { Say "  (승격 생략: $action)" 'DarkYellow'; return }
    $a = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $Kit.Elev, (Join-Path $Kit.Dir 'elevated.ps1'), '-Action', $action)
    if ($dir) { $a += @('-Dir', $dir) }
    & powershell @a 2>&1 | ForEach-Object { "  $_" } | Write-Host
}

# ---- capture start/stop
function Start-Captures($t) {
    $caps = @($t.Caps | Where-Object { $_ })
    $script:Procs = @(); $script:StopDirs = @(); $script:NextFile = $null
    $script:BtRunning = $false
    if ($caps -contains 'pointer' -or $caps -contains 'events') {
        $d = Join-Path $script:Raw 'probe'; New-Item -ItemType Directory -Force $d | Out-Null
        $mode = if ($caps -contains 'pointer') { 'pointer' } else { 'events' }
        if (Test-Path $Kit.Probe) {
            $pa = @($mode, "`"$d`""); if ($t.Kbd) { $pa += '--kbd' }   # keyboards only where the tail button is tested
            $script:Procs += Start-Process -FilePath $Kit.Probe -ArgumentList $pa -PassThru
            $script:StopDirs += $d
            if ($mode -eq 'pointer') { $script:NextFile = Join-Path $d 'next' }
        } else { Say '  sp9probe.exe 없음: setup.ps1을 먼저 실행' 'Red' }
    }
    foreach ($k in 'battery', 'perf', 'sensors', 'brightness', 'presence', 'power-mode') {
        if ($caps -notcontains $k) { continue }
        $d = Join-Path $script:Raw 'sampler'; New-Item -ItemType Directory -Force $d | Out-Null
        $script:Procs += Start-Bg 'powershell' @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "`"$(Join-Path $Kit.Dir 'sampler.ps1')`"", '-Kind', $k, '-Dir', "`"$d`"")
        $script:StopDirs += $d
    }
    if ($caps -contains 'hid') {
        $d = Join-Path $script:Raw 'hid'
        if (Test-Path $Kit.Python) {
            $script:Procs += Start-Bg $Kit.Python @('-I', "`"$(Join-Path $Kit.Dir 'hidlog.py')`"", '--out', "`"$d`"")
            $script:StopDirs += $d
        } else { Say '  Python venv 없음: setup.ps1' 'Red' }
    }
    if ($caps -contains 'hid-elev' -and -not $NoElevated) {
        $d = Join-Path $script:Raw 'hid-elev'; New-Item -ItemType Directory -Force $d | Out-Null
        # run-elevated waits for the elevated script, so the wrapper itself runs in the background; it stops via the stop file
        $script:Procs += Start-Bg 'powershell' @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "`"$($Kit.Elev)`"", "`"$(Join-Path $Kit.Dir 'elevated.ps1')`"", '-Action', 'hidlog', '-Dir', "`"$d`"")
        $script:StopDirs += $d
    }
    if ($caps -contains 'bt' -and -not $NoElevated) {
        if (Test-Path $Kit.Wprp) { Invoke-Elevated 'bt-start'; $script:BtRunning = $true }
        else { Say '  BluetoothStack.wprp 없음: setup.ps1' 'Red' }
    }
    # Wait until every backend has written its first data row (WinRT/Get-Counter take a few seconds to start),
    # so the first step is covered.
    $want = @()
    if ($caps -contains 'pointer' -or $caps -contains 'events') { $want += Join-Path $script:Raw 'probe\events.csv' }
    foreach ($k in 'battery', 'perf', 'sensors', 'brightness', 'presence', 'power-mode') { if ($caps -contains $k) { $want += Join-Path $script:Raw "sampler\$k.csv" } }
    $deadline = (Get-Date).AddSeconds(20)
    while ((Get-Date) -lt $deadline) {
        $ready = @($want | Where-Object { (Test-Path $_) -and @(Get-Content $_ -TotalCount 2 -ErrorAction SilentlyContinue).Count -ge 2 })
        if ($ready.Count -ge $want.Count) { break }
        Start-Sleep -Milliseconds 250
    }
    Start-Sleep -Milliseconds 500
}

function Stop-Captures($t) {
    foreach ($d in $script:StopDirs | Select-Object -Unique) { New-Item -ItemType File -Force (Join-Path $d 'stop') | Out-Null }
    $deadline = (Get-Date).AddSeconds(10)
    foreach ($p in $script:Procs) {
        $left = [int][math]::Max(1, ($deadline - (Get-Date)).TotalSeconds)
        try { $p | Wait-Process -Timeout $left -ErrorAction Stop } catch { try { $p | Stop-Process -Force -ErrorAction SilentlyContinue } catch {} }
    }
    if ($script:BtRunning) { Invoke-Elevated 'bt-stop' (Join-Path $script:Raw 'bt'); $script:BtRunning = $false }
    if (@($t.Caps) -contains 'winevents') { Export-WinEvents (Join-Path $script:Raw 'win') $script:T0 }
}

# ---- event log
function Export-WinEvents([string]$dir, [datetime]$since) {
    New-Item -ItemType Directory -Force $dir | Out-Null
    $out = New-Object Collections.Generic.List[string]; $out.Add('t,log,id,what,value')
    $what = @{ 506 = '모던 스탠바이 진입'; 507 = '모던 스탠바이 복귀'; 42 = '절전 진입'; 107 = '절전 복귀'; 1 = '절전 해제(Power-Troubleshooter)'; 109 = '종료 시작'; 41 = '비정상 종료'; 105 = '전원 원본 변경' }
    $ev = Get-WinEvent -FilterHashtable @{ LogName = 'System'; StartTime = $since } -ErrorAction SilentlyContinue |
        Where-Object { ($_.ProviderName -match 'Kernel-Power|Power-Troubleshooter') -and $what.ContainsKey($_.Id) }
    foreach ($e in $ev) { $out.Add(('{0},System,{1},{2},' -f ([DateTimeOffset]$e.TimeCreated).ToUnixTimeMilliseconds() / 1000, $e.Id, $what[$e.Id])) }
    $logs = Get-WinEvent -ListLog 'Microsoft-Surface-*' -ErrorAction SilentlyContinue | Where-Object { $_.RecordCount }
    foreach ($l in $logs) {
        $short = $l.LogName -replace '^Microsoft-Surface-|/Operational$', ''
        foreach ($e in Get-WinEvent -FilterHashtable @{ LogName = $l.LogName; StartTime = $since } -ErrorAction SilentlyContinue) {
            $t = ([DateTimeOffset]$e.TimeCreated).ToUnixTimeMilliseconds() / 1000
            if ($e.Message -match 'Channel Index=(\d+), ChannelInstance=(\d+), Value=(\d+)') {
                $out.Add(('{0},{1},{2},SMF ch{3}/{4},{5}' -f $t, $short, $e.Id, $Matches[1], $Matches[2], $Matches[3]))
            } else {
                $m = ($e.Message -split "`r?`n")[0] -replace '[,]', ';'
                if ($m.Length -gt 90) { $m = $m.Substring(0, 90) }
                $out.Add(('{0},{1},{2},{3},' -f $t, $short, $e.Id, $m))
            }
        }
    }
    [IO.File]::WriteAllLines((Join-Path $dir 'winevents.csv'), $out, $Utf8)
}

# ---- snapshots
function Save-Info([string]$name, [string]$phase) {
    $d = Join-Path $script:Raw 'info'; New-Item -ItemType Directory -Force $d | Out-Null
    $f = Join-Path $d "$name-$phase.txt"
    $txt = switch ($name) {
        'display' { & $Kit.Probe display }
        'pointerdevs' { & $Kit.Probe devices }
        'hidcaps-ble' {
            if ($NoElevated) { '(승격 생략)' } else {
                & powershell -NoProfile -ExecutionPolicy Bypass -File $Kit.Elev (Join-Path $Kit.Repo 'tools\windows\hid-caps.ps1') -Filter '0c0f' 2>&1 | ForEach-Object { "$_" }
            }
        }
        'camlist' {
            $ff = Get-Ffmpeg
            foreach ($c in 'Front', 'Rear') {
                "dshow Surface Camera ${c}:"
                if ($ff) { & $ff -hide_banner -list_options true -f dshow -i "video=Surface Camera $c" 2>&1 | ForEach-Object { "$_" } | Select-String 'pixel_format=nv12' | ForEach-Object { '  ' + ($_.Line -replace '^\[.*?\]\s*', '') } }
            }
            'WinRT frame sources:'
            & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $Kit.Dir 'camera-sources.ps1') 2>&1 | ForEach-Object { "$_" } |
                Select-String -Pattern '^group|open kind=|open failed|^\s+\d+x\d+' | ForEach-Object { $_.Line }
        }
        'audiodev' { & $Kit.Probe volume list; Get-PnpDevice -PresentOnly -Class AudioEndpoint -ErrorAction SilentlyContinue | ForEach-Object { "endpoint: $($_.FriendlyName) [$($_.Status)]" } }
        'btdevs' { Get-PnpDevice -PresentOnly -Class Bluetooth -ErrorAction SilentlyContinue | Sort-Object FriendlyName | ForEach-Object { "$($_.Status) $($_.FriendlyName)" } | Select-Object -Unique }
        'wlan' {
            # netsh wlan needs Location services (a setting this kit does not change); fall back to the adapter link speeds.
            $w = netsh wlan show interfaces 2>$null | Where-Object { $_ -match '^\s*(Description|State|Radio type|Band|Channel|Receive rate|Transmit rate|Signal|Authentication|Cipher|설명|상태|라디오 종류|대역|채널|수신 속도|전송 속도|신호|인증)\s*:' } | ForEach-Object { $_.Trim() }
            if ($w) { $w } else {
                '(netsh wlan: 위치 서비스가 꺼져 있어 대역·채널·신호를 못 읽음. 어댑터 링크 속도만)'
                Get-NetAdapter -Physical -ErrorAction SilentlyContinue | Where-Object { $_.MediaType -match '802.11' -or $_.PhysicalMediaType -match '802.11' } | ForEach-Object {
                    "adapter: $($_.InterfaceDescription), status $($_.Status), link rx $([math]::Round($_.ReceiveLinkSpeed / 1e6)) / tx $([math]::Round($_.TransmitLinkSpeed / 1e6)) Mbit/s, driver $($_.DriverVersion)"
                }
            }
            # driver capabilities (no location permission needed) and the signal bars of the connected profile (profile name not printed)
            netsh wlan show drivers 2>$null | Where-Object { $_ -match '^\s*(Driver|Version|Radio types supported|드라이버|버전|지원되는 무선 유형)\s*:' } | ForEach-Object { $_.Trim() }
            try {
                $null = [Windows.Networking.Connectivity.NetworkInformation, Windows.Networking.Connectivity, ContentType = WindowsRuntime]
                $cp = [Windows.Networking.Connectivity.NetworkInformation]::GetInternetConnectionProfile()
                if ($cp -and $cp.IsWlanConnectionProfile) { "signal bars: $($cp.GetSignalBars()) / 5" }
            } catch {}
        }
        'brightness' {
            $m = Get-CimInstance -Namespace root\wmi -ClassName WmiMonitorBrightness -ErrorAction SilentlyContinue | Select-Object -First 1
            if ($m) {
                $lv = @($m.Level)
                "현재 밝기 $($m.CurrentBrightness)%, 단계 $($m.Levels)개 ($(($lv | Measure-Object -Minimum).Minimum)..$(($lv | Measure-Object -Maximum).Maximum))"
            }
        }
        'sam-events' { & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $Kit.Repo 'tools\windows\sam-events.ps1') }
        'penbattery' {
            $pen = Get-PnpDevice -PresentOnly -ErrorAction SilentlyContinue | Where-Object { $_.FriendlyName -match 'Slim Pen' } | Select-Object -First 1
            if ($pen) {
                $b = (Get-PnpDeviceProperty -InstanceId $pen.InstanceId -KeyName '{104EA319-6EE2-4701-BD47-8DDBF425BBE5} 2' -ErrorAction SilentlyContinue).Data
                "펜 배터리 (PnP {104EA319…} 2): $b %"
                $hid = @(Get-PnpDevice -ErrorAction SilentlyContinue | Where-Object { $_.InstanceId -match '^HID\\.*0C0F' })
                "펜 BLE HID 컬렉션: " + (($hid | ForEach-Object { "$($_.FriendlyName)=$($_.Status)" }) -join ', ')
            } else { '펜이 페어링되어 있지 않다' }
        }
    }
    $lines = @(@($txt) | Where-Object { $null -ne $_ } | ForEach-Object { Protect-Text "$_" })
    if (-not $lines) { $lines = @("($name`: 출력 없음)") }
    try { [IO.File]::WriteAllLines($f, [string[]]$lines, $Utf8) } catch { Say "  스냅숏 $name 저장 실패: $($_.Exception.Message)" 'DarkYellow' }
}

# ---- actions (a step's Run)
function Invoke-Stress([int]$sec) {
    if ($DryRun) { $sec = $StepSeconds }
    Say "  부하 ${sec}초 ($env:NUMBER_OF_PROCESSORS 스레드)" 'DarkGray'
    & $Kit.Probe stress $sec | Out-Null
}

function Invoke-Audio([string]$cmd, [int]$volume, [string]$extra = '', [string]$tag = '') {
    if ($DryRun -and -not $AllowSound) { Say "  (dry-run: 소리 내는 단계 생략: $cmd)" 'DarkYellow'; return }
    $d = Join-Path $script:Raw 'audio'; New-Item -ItemType Directory -Force $d | Out-Null
    if ($cmd -ne 'record') {
        Say "  스피커 볼륨 ${volume}%로 재생" 'DarkGray'
        & $Kit.Probe volume set Omnisonic ($volume / 100).ToString($Inv) 0 | Out-Null
    }
    $a = @('-I', '-X', 'utf8', (Join-Path $Kit.Dir 'audio.py'), $cmd, '--out', $d) + ($extra -split ' ' | Where-Object { $_ })
    if ($tag) { $a += @('--tag', $tag) }
    & $Kit.Python @a | Out-Null
}

function Invoke-Cam([string]$cam, [string]$size, [int]$fps, [int]$sec, [string]$Tag = '', [switch]$Show, [switch]$Sharp) {
    $ff = Get-Ffmpeg
    $d = Join-Path $script:Raw 'cam'; New-Item -ItemType Directory -Force $d | Out-Null
    if (-not $ff) { [IO.File]::AppendAllText((Join-Path $d 'capture.txt'), "ffmpeg 없음`n", $Utf8); return }
    $name = "$cam-$size" + $(if ($Tag) { "-$Tag" })
    $jpg = Join-Path $d "$name.jpg"
    $log = & $ff -hide_banner -f dshow -video_size $size -framerate $fps -i "video=Surface Camera $cam" -t $sec -vf showinfo -f null NUL -frames:v 1 -update 1 -y $jpg 2>&1 | ForEach-Object { "$_" }
    $pts = @($log | Select-String 'pts_time:([\d.]+)' | ForEach-Object { [double]$_.Matches[0].Groups[1].Value })
    $fmt = ($log | Select-String 'Stream #0:0: Video: (.*)' | Select-Object -First 1 | ForEach-Object { $_.Matches[0].Groups[1].Value }) -replace ', start [\d.]+', ''
    $line = if ($pts.Count -gt 2) {
        "$cam $size@$fps 요청: $($pts.Count) 프레임, 실측 {0:F1} fps, 형식 $fmt" -f (($pts.Count - 1) / ($pts[-1] - $pts[0]))
    } else {
        $err = ($log | Select-String -Pattern 'error|could not|failed' | Select-Object -First 2 | ForEach-Object { $_.Line }) -join ' / '
        "$cam $size@$fps 요청: 프레임 없음 ($err)"
    }
    if ($Sharp -and (Test-Path $jpg)) {
        $pgm = Join-Path $d "$name.pgm"
        & $ff -hide_banner -loglevel error -y -i $jpg -pix_fmt gray $pgm 2>$null
        $s = & $Kit.Python -I -c "import numpy as n,sys;b=open(sys.argv[1],'rb').read();p=b.split(maxsplit=4);w,h=int(p[1]),int(p[2]);a=n.frombuffer(p[4][-w*h:],n.uint8).reshape(h,w).astype(float);l=a[1:-1,1:-1]*4-a[:-2,1:-1]-a[2:,1:-1]-a[1:-1,:-2]-a[1:-1,2:];print(round(l.var(),1))" $pgm
        $line += ", 선명도(라플라시안 분산) $s"
    }
    Say "  $line" 'DarkGray'
    [IO.File]::AppendAllText((Join-Path $d 'capture.txt'), "$line`n", $Utf8)
    [IO.File]::AppendAllText((Join-Path $d 'value.val'), "$($line -replace ' 요청:', ':' -replace ', 형식.*?(?=, 선명도|$)', ''); ", $Utf8)
    if ($Show -and -not $DryRun -and (Test-Path $jpg)) { Invoke-Item $jpg }
}

function Invoke-CamAll {
    $ff = Get-Ffmpeg
    if (-not $ff) { return }
    foreach ($c in 'Front', 'Rear') {
        $modes = & $ff -hide_banner -list_options true -f dshow -i "video=Surface Camera $c" 2>&1 | ForEach-Object { "$_" } |
            Select-String 'pixel_format=nv12\s+min s=(\d+x\d+) fps=\S+ max s=\S+ fps=(\d+)' | ForEach-Object { , @($_.Matches[0].Groups[1].Value, [int]$_.Matches[0].Groups[2].Value) }
        foreach ($m in $modes) { Invoke-Cam $c $m[0] $m[1] 2 }
    }
}

function Invoke-Iperf {
    $d = Join-Path $script:Raw 'net'; New-Item -ItemType Directory -Force $d | Out-Null
    $ip = Get-Command iperf3 -ErrorAction SilentlyContinue
    if (-not $ip -or $DryRun) { [IO.File]::WriteAllText((Join-Path $d 'iperf.txt'), "iperf3 생략(설치 안 됨 또는 dry-run)`n", $Utf8); return }
    $srv = Read-Host 'iperf3 서버 주소(없으면 Enter로 건너뜀)'
    if (-not $srv) { return }
    $res = foreach ($rev in @($false, $true)) {
        $a = @('-c', $srv, '-t', '10', '-J'); if ($rev) { $a += '-R' }
        $j = & $ip.Source @a 2>$null | ConvertFrom-Json
        $bps = $j.end.sum_received.bits_per_second
        "{0} {1:F0} Mbit/s" -f $(if ($rev) { '수신' } else { '송신' }), ($bps / 1e6)
    }
    [IO.File]::WriteAllLines((Join-Path $d 'iperf.txt'), [string[]]$res, $Utf8)
    [IO.File]::WriteAllText((Join-Path $d 'value.val'), ($res -join ', '), $Utf8)
}

# ---- pen preparation
function Test-PenAwake {
    $hid = @(Get-PnpDevice -PresentOnly -ErrorAction SilentlyContinue | Where-Object { $_.InstanceId -match '^HID\\.*0C0F' -and $_.Status -eq 'OK' })
    return $hid.Count -gt 0
}

# ---------------------------------------------------------------- result recording
function Write-Result($t, [string]$result, [string]$basis, [string]$setting, [string]$memo, [string]$summary, [string[]]$stepTexts) {
    New-Item -ItemType Directory -Force $Kit.Public | Out-Null
    $build = Get-WindowsBuild
    $res = Join-Path $Kit.Public 'results.md'
    if (-not (Test-Path $res)) {
        $head = @(
            '# 윈도우 기준 시험 결과', '',
            '[docs/test-plan.md](../../../docs/test-plan.md)의 ID별 윈도우 쪽 결과다. 형식은 `docs/test-results.md`와 같고 "커널" 칸에 윈도우 빌드를 적는다.',
            'ID별 수집 요약은 같은 폴더의 `<ID>.md`, 도구는 [tools/windows/baseline](../../../tools/windows/baseline/README.md)이다. 같은 ID를 다시 하면 줄을 더한다.', '',
            '| ID | 결과 | 근거 | 커널 | 설정 | 값·메모 |', '|---|---|---|---|---|---|')
        [IO.File]::WriteAllLines($res, [string[]]$head, $Utf8)
    }
    $esc = { param($s) (Protect-Text $s) -replace '\|', '/' -replace '[\r\n]+', ' ' }
    [IO.File]::AppendAllText($res, ("| {0} | {1} | {2} | {3} | {4} | {5} |`n" -f $t.Id, $result, $basis, $build, (& $esc $setting), (& $esc $memo)), $Utf8)

    $md = New-Object Collections.Generic.List[string]
    $md.Add("# $($t.Id) $($t.Name) (윈도우 기준)"); $md.Add('')
    $md.Add("| 항목 | 값 |"); $md.Add('|---|---|')
    $md.Add("| 윈도우 | $build |"); $md.Add("| 설정 | $(& $esc $setting) |"); $md.Add("| 결과 | $result ($basis) |"); $md.Add("| 값·메모 | $(& $esc $memo) |")
    $md.Add("| 리눅스 쪽 | [docs/test-plan.md](../../../docs/test-plan.md) $($t.Id) |"); $md.Add('')
    $md.Add('## 진행'); $md.Add('')
    $i = 1; foreach ($s in $stepTexts) { $md.Add("$i. $(($s -replace '^\[지우기\]\s*', ''))"); $i++ }
    $md.Add(''); $md.Add('## 수집 요약'); $md.Add('')
    foreach ($l in ($summary -split "`n")) { $md.Add((Protect-Text $l)) }
    [IO.File]::WriteAllLines((Join-Path $Kit.Public "$($t.Id).md"), $md, $Utf8)
}

# ---------------------------------------------------------------- run
function Invoke-Test($t) {
    Say ''
    Say ('=' * 70)
    Say "$($t.Id)  $($t.Name)" 'White'
    if ($Auto) {
        $why = Get-AutoSkip $t
        if ($why) { Say "  건너뜀: $why" 'DarkYellow'; $script:AutoLog += , @($t.Id, '건너뜀', $why); return }
        # the automatic part replaces the steps; captures and snapshots come from the test unless the plan overrides them
        $p = $AutoPlan[$t.Id]
        $t = $t.Clone()
        $t.Steps = $p.Steps
        if ($p.ContainsKey('Caps')) { $t.Caps = $p.Caps }
        if ($p.ContainsKey('Info')) { $t.Info = $p.Info }
        if ($p.Setting) { $t.Setting = $p.Setting }
        $t.Human = $p.Human
        $t.Do = "자동: " + ((@($p.Steps) | ForEach-Object { $_.Say }) -join ' → ')
    }
    if ($t.Na) { Say "  윈도우 대응 없음: $($t.Na)" 'DarkYellow'; return }
    if ($t.Needs -and -not $Auto) { Say "  필요: $($t.Needs)" }
    Say "  $($t.Do)" 'Gray'
    if ($t.Sound -and $Auto) { Say '  주의: 내장 스피커로 작은 시험 신호를 낸다(볼륨 20%, -24 dBFS). 끝나면 볼륨을 원래대로 돌린다.' 'Yellow' }
    elseif ($t.Sound -and -not $DryRun) { Say '  주의: 내장 스피커로 시험 신호를 낸다(-18 dBFS, 정해 둔 볼륨). 끝나면 볼륨을 원래대로 돌린다.' 'Yellow' }
    if ($t.Elevated -and -not $NoElevated) { Say '  일부 수집은 승격해서 돈다(run-elevated.ps1).' 'DarkGray' }

    # prerequisites
    $missing = @()
    if (-not (Test-Path $Kit.Probe)) { $missing += 'sp9probe.exe' }
    if (-not (Test-Path $Kit.Python)) { $missing += 'Python venv' }
    if ($missing) { Say "  준비 안 됨: $($missing -join ', ') → setup.ps1" 'Red'; if (-not $DryRun) { return } }
    if ($t.Needs -match '펜\(깨어 있음\)' -and -not $DryRun -and -not $Auto) {
        while (-not (Test-PenAwake)) {
            Say '  펜 BLE HID가 보이지 않는다(잠듦). 꼬리 버튼을 누르거나 펜으로 화면을 눌러 깨운 뒤 Enter(건너뛰려면 s).' 'Yellow'
            if ((Read-Host) -eq 's') { break }
        }
    }
    if (-not $DryRun -and -not $Auto -and -not $t.Manual) { if ((Read-Host '  시작하려면 Enter (건너뛰려면 s)') -eq 's') { return } }

    $stamp = (Get-Date -Format 'yyyyMMdd-HHmmss') + $(if ($Auto) { '-auto' })
    $base = if ($DryRun) { Join-Path $Kit.Private "_dryrun\$($t.Id)" } else { Join-Path $Kit.Private $t.Id }
    $script:Raw = Join-Path $base $stamp
    New-Item -ItemType Directory -Force $script:Raw | Out-Null
    $script:T0 = Get-Date
    $vol = $null
    if ($t.Sound) { $vol = (& $Kit.Probe volume get Omnisonic) -split ' ' }
    $stepTexts = @()
    try {
        foreach ($i in @($t.Info)) { if ($i) { Save-Info $i 'start' } }
        if (-not $t.Manual) { Start-Captures $t }
        $n = 0
        foreach ($s in $t.Steps) {
            $n++
            $say = if ($s -is [hashtable]) { $s.Say } else { $s }
            $stepTexts += $say
            Add-Marker $n $say
            if ($script:NextFile) { [IO.File]::WriteAllText((Join-Path (Split-Path $script:NextFile) 'step.txt'), "[$($t.Id) $n/$(@($t.Steps).Count)] $say", $Utf8) }
            Say "  [$n/$(@($t.Steps).Count)] $($say -replace '^\[지우기\]\s*', '')" 'White'
            if ($s -is [hashtable] -and $s.Run) { & $s.Run }
            else {
                $r = Wait-Next $(if ($t.Timeout) { $t.Timeout } else { 900 })
                if ($r -eq 'timeout') { Say '  (시간 초과, 다음 단계)' 'DarkYellow'; Add-Marker "$n-timeout" 'timeout' }
            }
        }
        Add-Marker 'end' 'end'
    } finally {
        if (-not $t.Manual) { Stop-Captures $t }
        foreach ($i in @($t.Info)) { if ($i) { Save-Info $i 'end' } }
        if ($vol) { & $Kit.Probe volume set Omnisonic $vol[0] $vol[1] | Out-Null; Say "  스피커 볼륨을 $([math]::Round([double]::Parse($vol[0], $Inv) * 100))%로 되돌렸다" 'DarkGray' }
    }

    # summary
    $sum = & $Kit.Python -I -X utf8 (Join-Path $Kit.Dir 'summarize.py') --test $t.Id --raw $script:Raw 2>&1 | ForEach-Object { "$_" }
    $value = ($sum | Where-Object { $_ -like 'VALUE: *' } | Select-Object -Last 1) -replace '^VALUE: ', ''
    $body = ($sum | Where-Object { $_ -notlike 'VALUE: *' }) -join "`n"
    Say ''
    Write-Host (Protect-Text $body)
    Say "  원본: $script:Raw" 'DarkGray'
    if ($DryRun) { Say "  제안 값: $(Protect-Text $value)" 'Green'; return }

    if ($Auto) {
        $result = if ($value) { '측정' } else { '미확인' }
        $memo = $value
        if ($t.Human) { $memo = (@($value, "사람 확인 필요: $($t.Human)") | Where-Object { $_ }) -join '; ' }
        $setting = if ($t.Setting) { $t.Setting } else { '기본' }
        $steps = @($stepTexts) + @("(자동 실행 -Auto. 사람 확인 필요: $(if ($t.Human) { $t.Human } else { '없음' }))")
        Write-Result $t $result '측정(자동)' $setting $memo $body $steps
        $script:AutoLog += , @($t.Id, $result, $(if ($t.Human) { $t.Human } else { '' }))
        Say "  기록: hw/windows/baseline/results.md, hw/windows/baseline/$($t.Id).md" 'Green'
        return
    }

    # ask
    Say ''
    $r = Read-Host '  결과? 1 통과 / 2 부분 / 3 실패 / 4 미확인'
    $result = @{ '1' = '통과'; '2' = '부분'; '3' = '실패'; '4' = '미확인' }[$r]; if (-not $result) { $result = '미확인' }
    $defBasis = if ($value -and -not $t.Manual) { '측정' } else { '관찰' }
    $b = Read-Host "  근거? 1 측정 / 2 관찰 (Enter = $defBasis)"
    $basis = @{ '1' = '측정'; '2' = '관찰' }[$b]; if (-not $basis) { $basis = $defBasis }
    $defSet = if ($t.Setting) { $t.Setting } else { '기본' }
    $setting = Read-Host "  설정? (Enter = $defSet)"; if (-not $setting) { $setting = $defSet }
    Say "  제안 값: $(Protect-Text $value)" 'Green'
    $memo = Read-Host '  값·메모? Enter = 제안 그대로, +내용 = 제안 뒤에 덧붙임, 그 밖 = 대체'
    if (-not $memo) { $memo = $value } elseif ($memo.StartsWith('+')) { $memo = ($value + '; ' + $memo.Substring(1)).Trim('; ') }
    Write-Result $t $result $basis $setting $memo $body $stepTexts
    Say "  기록: hw/windows/baseline/results.md, hw/windows/baseline/$($t.Id).md" 'Green'
}

if ($List -or (-not $Test -and -not $Area -and -not $Auto)) {
    $area = ''
    foreach ($t in $Tests) {
        $a = $t.Id.Split('-')[0]
        if ($a -ne $area) { Write-Host ''; $area = $a }
        $tag = if ($t.Na) { '[대응 없음] ' + $t.Na } elseif ($t.Manual) { '[관찰만] ' + $t.Do } else { $t.Do }
        $flags = @(); if ($t.Elevated) { $flags += '승격' }; if ($t.Sound) { $flags += '소리' }; if ($t.Needs) { $flags += $t.Needs }
        Write-Host ("{0,-8} {1}" -f $t.Id, $t.Name) -NoNewline -ForegroundColor White
        Write-Host $(if ($flags) { "  ($($flags -join ', '))" } else { '' }) -ForegroundColor DarkGray
        Write-Host ("         {0}" -f $tag)
    }
    return
}

# Rebuild the probe when its source is newer (setup.ps1 is idempotent)
$src = Join-Path $Kit.Dir 'sp9probe.cs'
if (-not (Test-Path $Kit.Probe) -or -not (Test-Path $Kit.Python) -or (Get-Item $src).LastWriteTime -gt (Get-Item $Kit.Probe).LastWriteTime) {
    Say 'setup.ps1 실행(빌드·venv)' 'DarkGray'
    & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $Kit.Dir 'setup.ps1') | ForEach-Object { "  $_" } | Write-Host
}

$sel = @()
if ($Test) { $ids = $Test | ForEach-Object { $_ -split ',' } | ForEach-Object { $_.Trim().ToUpper() } | Where-Object { $_ }; $sel = $Tests | Where-Object { $ids -contains $_.Id } }
if ($Area) { $sel += $Tests | Where-Object { $_.Id -like "$($Area.ToUpper())-*" } }
if ($Auto -and -not $Test -and -not $Area) { $sel = $Tests }   # -Auto alone: every test, skipped where a person is needed
if (-not $sel) { Say '해당 ID 없음. -List로 확인' 'Red'; exit 1 }
$script:AutoLog = @()
foreach ($t in $sel) { Invoke-Test $t }
if ($Auto) {
    Say ''
    Say ('=' * 70)
    Say '자동 실행 요약' 'White'
    foreach ($r in $script:AutoLog) { Write-Host ("  {0,-8} {1,-4} {2}" -f $r[0], $r[1], $r[2]) }
}
