// EN/VI catalogs — zero embedded strings in components (UX_SPEC §10).
// Error codes come from the Rust taxonomy (error.rs); every code the UI
// can receive must have a localized entry (fallback = the code itself).

export type Lang = "en" | "vi";

type Dict = Record<string, string>;

const en: Dict = {
  "app.title": "MELOGRAPHY",
  "toolbar.open": "Open image…",
  "toolbar.export": "Export…",
  "toolbar.save": "Save recipe",
  "toolbar.saved": "Recipe saved",
  "toolbar.compare": "Compare",
  "toolbar.zoomIn": "Zoom in",
  "toolbar.zoomOut": "Zoom out",
  "toolbar.fit": "Fit",
  "toolbar.actual": "100%",
  "viewport.dropHere": "Open an image to begin — or drop one anywhere",
  "viewport.original": "original",
  "viewport.edited": "edited",
  "viewport.holdB": "hold B for original",
  "viewport.renderMs": "render",
  "viewport.clipping": "LUT domain clipping",
  "panel.lut": "LUT",
  "panel.lut.library": "Library",
  "panel.lut.empty": "No LUTs imported yet",
  "panel.lut.import": "Import .cube…",
  "panel.lut.remove": "Remove",
  "panel.lut.intensity": "Intensity",
  "panel.lut.insertion": "Insertion point",
  "panel.lut.none": "No LUT applied",
  "insertion.pre-film": "Pre-film",
  "insertion.film-input": "Film input",
  "insertion.print-output": "Print output",
  "insertion.finishing": "Finishing",
  "encoding.label.in": "Input encoding",
  "encoding.label.out": "Output encoding",
  "encoding.srgb-display": "sRGB (display)",
  "encoding.rec709-display-linear": "Rec.709 (linear)",
  "encoding.rec709-gamma2.4": "Rec.709 (gamma 2.4)",
  "encoding.display-p3-display": "Display P3 (display)",
  "encoding.acescct": "ACEScct",
  "encoding.acescc": "ACEScc",
  "encoding.agx-log": "AgX log (coming soon)",
  "encoding.custom-1d-shaper": "Custom 1D shaper (coming soon)",
  "export.title": "Export image",
  "export.format": "Format",
  "export.depth": "Bit depth",
  "export.quality": "Quality",
  "export.save": "Export",
  "export.cancel": "Cancel",
  "export.done": "Exported",
  "import.title": "Import LUT",
  "import.file": "File",
  "import.pick": "Choose .cube…",
  "import.provenance": "Provenance (optional)",
  "import.sourceUrl": "Source URL",
  "import.license": "License note",
  "import.doImport": "Import",
  "import.warning.monotonic": "non-monotonic 1D channel",
  "import.warning.tiny3d": "tiny 2×2×2 grid",
  "import.warning.domain": "domain extends outside [0,1]",
  "error.title": "Something needs attention",
  "error.dismiss": "Dismiss",
  "error.code.io.read": "The file could not be opened. It may have been moved, renamed, or you lack permission.",
  "error.code.io.unsupported": "This format is not supported. Supported: JPEG, PNG, TIFF, WebP (HEIC/AVIF experimental).",
  "error.code.io.decode": "The file is damaged or truncated and cannot be decoded.",
  "error.code.io.bomb": "The image exceeds the safety size limit (decompression bomb protection).",
  "error.code.lut.malformed": "The LUT file is not a valid .cube. Check the reported line.",
  "error.code.lut.missing": "The LUT referenced by this recipe is missing from the library. Re-import the original file.",
  "error.code.lut.checksum": "The stored LUT failed its integrity check. Re-import the original file.",
  "error.code.lut.encoding_unimplemented": "This LUT declares an encoding this build does not implement yet.",
  "error.code.lut.encoding_unknown": "Unknown encoding identifier.",
  "error.code.session.none": "No image is open.",
  "error.code.recipe.version": "The sidecar was written by a newer MELOGRAPHY. Update the app.",
  "error.code.export.failed": "Export failed. Check disk space and permissions.",
  "error.fallback": "An unexpected error occurred.",
  "state.dirty": "unsaved",
  "state.sidecar": "recipe",
  "units.percent": "%",
};

const vi: Dict = {
  "app.title": "MELOGRAPHY",
  "toolbar.open": "Mở ảnh…",
  "toolbar.export": "Xuất ảnh…",
  "toolbar.save": "Lưu công thức",
  "toolbar.saved": "Đã lưu công thức",
  "toolbar.compare": "So sánh",
  "toolbar.zoomIn": "Phóng to",
  "toolbar.zoomOut": "Thu nhỏ",
  "toolbar.fit": "Vừa khung",
  "toolbar.actual": "100%",
  "viewport.dropHere": "Mở một tấm ảnh để bắt đầu — hoặc kéo thả vào đây",
  "viewport.original": "gốc",
  "viewport.edited": "đã chỉnh",
  "viewport.holdB": "giữ B để xem gốc",
  "viewport.renderMs": "kết xuất",
  "viewport.clipping": "LUT đang cắt ngoài miền màu",
  "panel.lut": "LUT",
  "panel.lut.library": "Thư viện",
  "panel.lut.empty": "Chưa có LUT nào",
  "panel.lut.import": "Nhập .cube…",
  "panel.lut.remove": "Xóa",
  "panel.lut.intensity": "Cường độ",
  "panel.lut.insertion": "Vị trí chèn",
  "panel.lut.none": "Chưa áp LUT",
  "insertion.pre-film": "Trước phim",
  "insertion.film-input": "Đầu vào phim",
  "insertion.print-output": "Đầu ra in",
  "insertion.finishing": "Hoàn thiện",
  "encoding.label.in": "Mã màu đầu vào",
  "encoding.label.out": "Mã màu đầu ra",
  "encoding.srgb-display": "sRGB (hiển thị)",
  "encoding.rec709-display-linear": "Rec.709 (tuyến tính)",
  "encoding.rec709-gamma2.4": "Rec.709 (gamma 2.4)",
  "encoding.display-p3-display": "Display P3 (hiển thị)",
  "encoding.acescct": "ACEScct",
  "encoding.acescc": "ACEScc",
  "encoding.agx-log": "AgX log (sắp có)",
  "encoding.custom-1d-shaper": "Shaper 1D tùy chỉnh (sắp có)",
  "export.title": "Xuất ảnh",
  "export.format": "Định dạng",
  "export.depth": "Độ sâu bit",
  "export.quality": "Chất lượng",
  "export.save": "Xuất",
  "export.cancel": "Hủy",
  "export.done": "Đã xuất",
  "import.title": "Nhập LUT",
  "import.file": "Tệp",
  "import.pick": "Chọn .cube…",
  "import.provenance": "Nguồn gốc (không bắt buộc)",
  "import.sourceUrl": "URL nguồn",
  "import.license": "Ghi chú bản quyền",
  "import.doImport": "Nhập",
  "import.warning.monotonic": "kênh 1D không đơn điệu",
  "import.warning.tiny3d": "lưới 2×2×2 quá nhỏ",
  "import.warning.domain": "miền màu vượt ngoài [0,1]",
  "error.title": "Cần xử lý",
  "error.dismiss": "Đã hiểu",
  "error.code.io.read": "Không mở được tệp. Tệp có thể đã bị di chuyển, đổi tên, hoặc bạn thiếu quyền truy cập.",
  "error.code.io.unsupported": "Định dạng không được hỗ trợ. Hỗ trợ: JPEG, PNG, TIFF, WebP (HEIC/AVIF thử nghiệm).",
  "error.code.io.decode": "Tệp bị hỏng hoặc thiếu dữ liệu, không thể giải mã.",
  "error.code.io.bomb": "Ảnh vượt quá giới hạn an toàn về kích thước (chống bom giải nén).",
  "error.code.lut.malformed": "Tệp LUT không phải .cube hợp lệ. Xem dòng được báo lỗi.",
  "error.code.lut.missing": "LUT mà công thức tham chiếu không còn trong thư viện. Hãy nhập lại tệp gốc.",
  "error.code.lut.checksum": "LUT lưu trong thư viện không khớp kiểm tra toàn vẹn. Hãy nhập lại tệp gốc.",
  "error.code.lut.encoding_unimplemented": "LUT này khai báo một kiểu mã màu mà bản build chưa hỗ trợ.",
  "error.code.lut.encoding_unknown": "Định danh mã màu không hợp lệ.",
  "error.code.session.none": "Chưa mở ảnh nào.",
  "error.code.recipe.version": "Công thức được tạo bởi phiên bản MELOGRAPHY mới hơn. Hãy cập nhật ứng dụng.",
  "error.code.export.failed": "Xuất ảnh thất bại. Kiểm tra dung lượng ổ đĩa và quyền ghi.",
  "error.fallback": "Đã xảy ra lỗi không mong đợi.",
  "state.dirty": "chưa lưu",
  "state.sidecar": "công thức",
  "units.percent": "%",
};

const catalogs: Record<Lang, Dict> = { en, vi };

export let currentLang: Lang = "en";

export function setLang(lang: Lang): void {
  currentLang = lang;
}

export function t(key: string, params?: Record<string, string | number>): string {
  let s = catalogs[currentLang][key] ?? catalogs.en[key] ?? key;
  if (params) {
    for (const [k, v] of Object.entries(params)) {
      s = s.replace(`{${k}}`, String(v));
    }
  }
  return s;
}

/** Localized guidance for a Rust error code (error.rs taxonomy). */
export function tErrorCode(code: string, message: string): string {
  const key = `error.code.${code}`;
  const dict = catalogs[currentLang];
  const localized = dict[key] ?? catalogs.en[key];
  return localized ? `${localized} — ${message}` : `${message} [${code}]`;
}
