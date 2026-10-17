import math
import os
from PIL import Image, ImageDraw, ImageFilter

def create_keyboard_icon():
    # 512x512 고해상도로 렌더링한 후 256x256 등으로 리사이즈하여 고품질 안티에일리어싱 적용
    S = 512
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)

    # 1. 미세한 그림자 (Soft Drop Shadow)
    shadow = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    s_draw = ImageDraw.Draw(shadow)
    # 키보드 본체 그림자
    s_draw.rounded_rectangle(
        [40, 96, 472, 424],
        radius=48,
        fill=(0, 0, 0, 110)
    )
    shadow = shadow.filter(ImageFilter.GaussianBlur(16))
    img.alpha_composite(shadow)

    draw = ImageDraw.Draw(img)

    # 2. 키보드 본체 섀시 (Outer Body)
    # Fluent Dark Slate / Deep Blue 테두리 및 바디
    body_box = [40, 88, 472, 416]
    body_radius = 48
    draw.rounded_rectangle(body_box, radius=body_radius, fill=(30, 36, 52, 255), outline=(62, 74, 102, 255), width=6)

    # 섀시 상단 미세 하이라이트 라인
    draw.rounded_rectangle([44, 92, 468, 412], radius=44, outline=(75, 90, 122, 100), width=3)

    # 3. 키보드 내부 트레이/베벨 영역 (Darker recessed plate)
    plate_box = [62, 118, 450, 390]
    draw.rounded_rectangle(plate_box, radius=28, fill=(18, 22, 32, 255), outline=(36, 44, 62, 255), width=4)

    # 4. 키캡 색상 팔레트
    # 모던 기계식 키보드 테마:
    # 캡 윗면: 깨끗한 오프화이트 (#F4F7FC)
    # 캡 옆면(입체감): (#D0D7E5)
    # 액센트 키 (ESC, Enter): 모던 스카이블루 (#0078D4 / #38BDF8)
    def draw_keycap(box, radius=12, fill_top=(245, 248, 252, 255), fill_side=(200, 210, 225, 255), is_accent=False, is_space=False):
        x1, y1, x2, y2 = box
        # 측면/바닥 입체감 (높이 8px)
        draw.rounded_rectangle([x1, y1 + 6, x2, y2], radius=radius, fill=fill_side)
        # 상단 표면
        draw.rounded_rectangle([x1, y1, x2, y2 - 6], radius=radius, fill=fill_top)
        # 키캡 윗면 미세 광택 테두리
        if is_accent:
            draw.rounded_rectangle([x1 + 1, y1 + 1, x2 - 1, y2 - 7], radius=radius - 1, outline=(140, 215, 255, 180), width=2)
        else:
            draw.rounded_rectangle([x1 + 1, y1 + 1, x2 - 1, y2 - 7], radius=radius - 1, outline=(255, 255, 255, 200), width=2)

    # 행 1: 숫자열 느낌 (6개 키)
    r1_y1, r1_y2 = 138, 204
    keys_r1 = [
        (80, 130),    # ESC / Accent Key
        (140, 190),
        (200, 250),
        (260, 310),
        (320, 370),
        (380, 432),   # Backspace / Accent Key
    ]
    for i, (k_x1, k_x2) in enumerate(keys_r1):
        accent = (i == 0) # ESC 키는 액센트 블루
        if accent:
            draw_keycap([k_x1, r1_y1, k_x2, r1_y2], radius=14,
                        fill_top=(0, 140, 240, 255), fill_side=(0, 100, 185, 255), is_accent=True)
        elif i == 5: # Backspace
            draw_keycap([k_x1, r1_y1, k_x2, r1_y2], radius=14,
                        fill_top=(70, 80, 105, 255), fill_side=(45, 52, 70, 255))
        else:
            draw_keycap([k_x1, r1_y1, k_x2, r1_y2], radius=14)

    # 행 2: 문자열 (5개 키)
    r2_y1, r2_y2 = 218, 284
    keys_r2 = [
        (80, 142),    # Tab / Mod
        (152, 212),
        (222, 282),
        (292, 352),
        (362, 432),   # Enter / Return
    ]
    for i, (k_x1, k_x2) in enumerate(keys_r2):
        if i == 0 or i == 4:
            draw_keycap([k_x1, r2_y1, k_x2, r2_y2], radius=14,
                        fill_top=(70, 80, 105, 255), fill_side=(45, 52, 70, 255))
        else:
            draw_keycap([k_x1, r2_y1, k_x2, r2_y2], radius=14)

    # 행 3: 하단열 (스페이스바 + 모디파이어)
    r3_y1, r3_y2 = 298, 364
    # 좌측 Ctrl/Alt
    draw_keycap([80, r3_y1, 142, r3_y2], radius=14,
                fill_top=(70, 80, 105, 255), fill_side=(45, 52, 70, 255))
    # 길고 명확한 스페이스바! (키보드의 상징)
    draw_keycap([154, r3_y1, 358, r3_y2], radius=14,
                fill_top=(245, 248, 252, 255), fill_side=(200, 210, 225, 255), is_space=True)
    # 우측 Mod
    draw_keycap([370, r3_y1, 432, r3_y2], radius=14,
                fill_top=(70, 80, 105, 255), fill_side=(45, 52, 70, 255))

    # 5. 우측 하단 채터링 픽스 쉴드/배지 (Chatter Fix Badge)
    # 원형 에메랄드/민트 녹색 배지에 선명한 흰색 체크마크 (Fix & Shield 상징)
    badge_center = (410, 386)
    badge_radius = 64

    # 배지 그림자
    b_shadow = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    b_s_draw = ImageDraw.Draw(b_shadow)
    b_s_draw.ellipse(
        [badge_center[0] - badge_radius, badge_center[1] - badge_radius + 6,
         badge_center[0] + badge_radius, badge_center[1] + badge_radius + 6],
        fill=(0, 0, 0, 140)
    )
    b_shadow = b_shadow.filter(ImageFilter.GaussianBlur(10))
    img.alpha_composite(b_shadow)

    draw = ImageDraw.Draw(img)

    # 외곽 화이트/섀시 링
    draw.ellipse(
        [badge_center[0] - badge_radius, badge_center[1] - badge_radius,
         badge_center[0] + badge_radius, badge_center[1] + badge_radius],
        fill=(16, 185, 129, 255), # Emerald Green (#10B981)
        outline=(255, 255, 255, 255),
        width=8
    )

    # 배지 내부 그라데이션 광택 느낌 (상단 타원)
    draw.ellipse(
        [badge_center[0] - badge_radius + 6, badge_center[1] - badge_radius + 6,
         badge_center[0] + badge_radius - 6, badge_center[1] + badge_radius - 6],
        outline=(110, 231, 183, 200),
        width=3
    )

    # 선명하고 깔끔한 체크마크 (✓) 그리기
    # 체크마크 좌표: 꺾이는 점(400, 404), 좌측 시작(380, 384), 우측 끝(440, 360)
    check_coords = [
        (badge_center[0] - 26, badge_center[1] - 2),
        (badge_center[0] - 8, badge_center[1] + 18),
        (badge_center[0] + 28, badge_center[1] - 20)
    ]
    draw.line(check_coords, fill=(255, 255, 255, 255), width=12, joint="round")

    # 6. 고품질 리사이즈 (256x256 베이스)
    base_256 = img.resize((256, 256), Image.Resampling.LANCZOS)
    return base_256

def generate_svg():
    svg_content = '''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256" width="100%" height="100%">
  <defs>
    <!-- Background Shadow Filter -->
    <filter id="drop-shadow" x="-10%" y="-10%" width="130%" height="130%">
      <feDropShadow dx="0" dy="6" stdDeviation="6" flood-color="#000000" flood-opacity="0.35"/>
    </filter>
    <filter id="badge-shadow" x="-20%" y="-20%" width="140%" height="140%">
      <feDropShadow dx="0" dy="4" stdDeviation="4" flood-color="#000000" flood-opacity="0.4"/>
    </filter>

    <!-- Gradients -->
    <linearGradient id="bodyGrad" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="#2D3548"/>
      <stop offset="100%" stop-color="#1B202D"/>
    </linearGradient>

    <linearGradient id="plateGrad" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="#131620"/>
      <stop offset="100%" stop-color="#191E2A"/>
    </linearGradient>

    <linearGradient id="keyWhiteTop" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="#FFFFFF"/>
      <stop offset="100%" stop-color="#EBF0F7"/>
    </linearGradient>

    <linearGradient id="keyModTop" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="#55627D"/>
      <stop offset="100%" stop-color="#3F495E"/>
    </linearGradient>

    <linearGradient id="keyAccentTop" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="#38BDF8"/>
      <stop offset="100%" stop-color="#0284C7"/>
    </linearGradient>

    <linearGradient id="badgeGrad" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#34D399"/>
      <stop offset="100%" stop-color="#059669"/>
    </linearGradient>
  </defs>

  <!-- Keyboard Housing -->
  <rect x="20" y="44" width="216" height="164" rx="24" fill="url(#bodyGrad)" stroke="#4A5568" stroke-width="2.5" filter="url(#drop-shadow)"/>
  <rect x="22" y="46" width="212" height="160" rx="22" fill="none" stroke="#64748B" stroke-width="1" opacity="0.4"/>

  <!-- Keyboard Plate (Recessed Area) -->
  <rect x="31" y="59" width="194" height="136" rx="14" fill="url(#plateGrad)" stroke="#2D3748" stroke-width="2"/>

  <!-- Row 1 Keys -->
  <!-- ESC (Accent) -->
  <rect x="40" y="72" width="25" height="30" rx="6" fill="#0369A1"/>
  <rect x="40" y="69" width="25" height="30" rx="6" fill="url(#keyAccentTop)"/>

  <!-- Keys 1..4 -->
  <rect x="70" y="72" width="25" height="30" rx="6" fill="#CBD5E1"/>
  <rect x="70" y="69" width="25" height="30" rx="6" fill="url(#keyWhiteTop)"/>

  <rect x="100" y="72" width="25" height="30" rx="6" fill="#CBD5E1"/>
  <rect x="100" y="69" width="25" height="30" rx="6" fill="url(#keyWhiteTop)"/>

  <rect x="130" y="72" width="25" height="30" rx="6" fill="#CBD5E1"/>
  <rect x="130" y="69" width="25" height="30" rx="6" fill="url(#keyWhiteTop)"/>

  <rect x="160" y="72" width="25" height="30" rx="6" fill="#CBD5E1"/>
  <rect x="160" y="69" width="25" height="30" rx="6" fill="url(#keyWhiteTop)"/>

  <!-- Backspace (Mod) -->
  <rect x="190" y="72" width="26" height="30" rx="6" fill="#29303D"/>
  <rect x="190" y="69" width="26" height="30" rx="6" fill="url(#keyModTop)"/>

  <!-- Row 2 Keys -->
  <!-- Tab (Mod) -->
  <rect x="40" y="112" width="31" height="30" rx="6" fill="#29303D"/>
  <rect x="40" y="109" width="31" height="30" rx="6" fill="url(#keyModTop)"/>

  <!-- Keys Q..E -->
  <rect x="76" y="112" width="30" height="30" rx="6" fill="#CBD5E1"/>
  <rect x="76" y="109" width="30" height="30" rx="6" fill="url(#keyWhiteTop)"/>

  <rect x="111" y="112" width="30" height="30" rx="6" fill="#CBD5E1"/>
  <rect x="111" y="109" width="30" height="30" rx="6" fill="url(#keyWhiteTop)"/>

  <rect x="146" y="112" width="30" height="30" rx="6" fill="#CBD5E1"/>
  <rect x="146" y="109" width="30" height="30" rx="6" fill="url(#keyWhiteTop)"/>

  <!-- Enter (Mod) -->
  <rect x="181" y="112" width="35" height="30" rx="6" fill="#29303D"/>
  <rect x="181" y="109" width="35" height="30" rx="6" fill="url(#keyModTop)"/>

  <!-- Row 3 Keys -->
  <!-- Ctrl (Mod) -->
  <rect x="40" y="152" width="31" height="30" rx="6" fill="#29303D"/>
  <rect x="40" y="149" width="31" height="30" rx="6" fill="url(#keyModTop)"/>

  <!-- SPACEBAR (Wide Accent/White Key) -->
  <rect x="76" y="152" width="102" height="30" rx="6" fill="#CBD5E1"/>
  <rect x="76" y="149" width="102" height="30" rx="6" fill="url(#keyWhiteTop)"/>

  <!-- Alt (Mod) -->
  <rect x="183" y="152" width="33" height="30" rx="6" fill="#29303D"/>
  <rect x="183" y="149" width="33" height="30" rx="6" fill="url(#keyModTop)"/>

  <!-- Fix / Shield Badge (Right Bottom) -->
  <g filter="url(#badge-shadow)">
    <circle cx="205" cy="193" r="32" fill="url(#badgeGrad)" stroke="#FFFFFF" stroke-width="4"/>
    <circle cx="205" cy="193" r="29" fill="none" stroke="#A7F3D0" stroke-width="1.5" opacity="0.6"/>
    <!-- Checkmark Path -->
    <path d="M192 192 L201 202 L219 183" fill="none" stroke="#FFFFFF" stroke-width="5.5" stroke-linecap="round" stroke-linejoin="round"/>
  </g>
</svg>'''
    return svg_content

def make_paused_icon(img):
    """Gray, slightly faded copy for the tray while protection is paused/off."""
    gray = img.convert("LA").convert("RGBA")
    alpha = img.getchannel("A").point(lambda a: int(a * 0.75))
    gray.putalpha(alpha)
    return gray


if __name__ == "__main__":
    os.makedirs("assets", exist_ok=True)
    os.makedirs("packaging", exist_ok=True)

    # 1. SVG 생성
    svg_path = "assets/icon.svg"
    with open(svg_path, "w", encoding="utf-8") as f:
        f.write(generate_svg())
    print(f"Generated {svg_path}")

    # 2. 고화질 256x256 PNG 생성
    img_256 = create_keyboard_icon()
    png_path = "assets/icon.png"
    img_256.save(png_path, "PNG", optimize=True)
    print(f"Generated {png_path} ({os.path.getsize(png_path)} bytes)")

    # 3. Windows Multi-Size ICO 생성 (16, 24, 32, 48, 64, 128, 256)
    ico_path = "packaging/app_icon.ico"
    sizes = [(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)]
    img_256.save(ico_path, format="ICO", sizes=sizes)
    print(f"Generated {ico_path} ({os.path.getsize(ico_path)} bytes)")

    # 4. 보호 꺼짐/일시 중지용 회색 트레이 아이콘 (리소스 ID 2)
    paused_path = "packaging/app_icon_paused.ico"
    make_paused_icon(img_256).save(paused_path, format="ICO", sizes=sizes)
    print(f"Generated {paused_path} ({os.path.getsize(paused_path)} bytes)")
