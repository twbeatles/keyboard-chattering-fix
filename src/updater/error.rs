//! Update errors as semantic codes, so the UI can show them in the active
//! language (ISSUE-006). Technical details (I/O or HTTP errors) are carried
//! separately and appended verbatim.

use crate::i18n::Language;

/// What went wrong, independent of the display language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateErrorKind {
    ManifestInvalid,
    SignatureInvalid,
    FileInfoInvalid,
    TagInvalid,
    LocationInvalid,
    SizeOutOfRange,
    ExpiryInvalid,
    Expired,
    VersionInvalid,
    NotPublished,
    ManifestDownload,
    Download,
    HashMismatch,
    SaveFailed,
    ExePath,
    HelperCorrupt,
    HelperVersionMismatch,
    HelperMissing,
    HelperStage,
    HelperLaunch,
    Encoding,
}

impl UpdateErrorKind {
    /// Every kind, for the bilingual completeness test.
    #[cfg(test)]
    pub const ALL: [UpdateErrorKind; 21] = [
        Self::ManifestInvalid,
        Self::SignatureInvalid,
        Self::FileInfoInvalid,
        Self::TagInvalid,
        Self::LocationInvalid,
        Self::SizeOutOfRange,
        Self::ExpiryInvalid,
        Self::Expired,
        Self::VersionInvalid,
        Self::NotPublished,
        Self::ManifestDownload,
        Self::Download,
        Self::HashMismatch,
        Self::SaveFailed,
        Self::ExePath,
        Self::HelperCorrupt,
        Self::HelperVersionMismatch,
        Self::HelperMissing,
        Self::HelperStage,
        Self::HelperLaunch,
        Self::Encoding,
    ];

    /// (Korean, English, Japanese) text for this kind.
    fn texts(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::ManifestInvalid => (
                "업데이트 매니페스트가 올바르지 않습니다.",
                "The update manifest is invalid.",
                "更新マニフェストが正しくありません。",
            ),
            Self::SignatureInvalid => (
                "업데이트 서명 또는 형식 검증에 실패했습니다.",
                "The update signature or format could not be verified.",
                "更新の署名または形式を検証できませんでした。",
            ),
            Self::FileInfoInvalid => (
                "업데이트 파일 정보가 올바르지 않습니다.",
                "The update file information is invalid.",
                "更新ファイルの情報が正しくありません。",
            ),
            Self::TagInvalid => (
                "업데이트 태그 정보가 올바르지 않습니다.",
                "The update release tag is invalid.",
                "更新のタグ情報が正しくありません。",
            ),
            Self::LocationInvalid => (
                "업데이트 파일 위치가 올바르지 않습니다.",
                "The update file location is not trusted.",
                "更新ファイルの場所は信頼できません。",
            ),
            Self::SizeOutOfRange => (
                "업데이트 파일 크기가 허용 범위를 벗어났습니다.",
                "The update file size is outside the allowed range.",
                "更新ファイルのサイズが許容範囲外です。",
            ),
            Self::ExpiryInvalid => (
                "업데이트 매니페스트의 유효기간 형식이 올바르지 않습니다.",
                "The update manifest expiry date is malformed.",
                "更新マニフェストの有効期限の形式が正しくありません。",
            ),
            Self::Expired => (
                "업데이트 매니페스트가 만료되었습니다.",
                "The update manifest has expired.",
                "更新マニフェストの期限が切れています。",
            ),
            Self::VersionInvalid => (
                "업데이트 버전 정보가 올바르지 않습니다.",
                "The update version number is invalid.",
                "更新のバージョン番号が正しくありません。",
            ),
            Self::NotPublished => (
                "게시된 업데이트가 아직 없습니다. 첫 릴리스 이후에 다시 확인해 주세요.",
                "No update has been published yet. Please check again after the first release.",
                "まだ公開された更新はありません。最初のリリース後に再度ご確認ください。",
            ),
            Self::ManifestDownload => (
                "업데이트 정보를 내려받지 못했습니다.",
                "Could not download the update information.",
                "更新情報をダウンロードできませんでした。",
            ),
            Self::Download => (
                "업데이트 파일을 내려받지 못했습니다.",
                "Could not download the update.",
                "更新ファイルをダウンロードできませんでした。",
            ),
            Self::HashMismatch => (
                "업데이트 파일 해시가 일치하지 않습니다.",
                "The downloaded update failed its integrity check.",
                "ダウンロードした更新の整合性チェックに失敗しました。",
            ),
            Self::SaveFailed => (
                "업데이트 파일을 저장하지 못했습니다.",
                "Could not save the update file.",
                "更新ファイルを保存できませんでした。",
            ),
            Self::ExePath => (
                "현재 실행 파일 경로를 확인하지 못했습니다.",
                "Could not determine the program's own location.",
                "プログラム自身の場所を特定できませんでした。",
            ),
            Self::HelperCorrupt => (
                "kcfix-updater.exe 헬퍼 파일이 손상되었습니다. 앱과 같은 폴더에 정상 파일을 두세요.",
                "kcfix-updater.exe is damaged. Put an intact copy next to the program.",
                "kcfix-updater.exe ヘルパーファイルが壊れています。正常なファイルをプログラムと同じフォルダーに置いてください。",
            ),
            Self::HelperVersionMismatch => (
                "kcfix-updater.exe 헬퍼가 이 프로그램과 호환되지 않습니다. 릴리스 zip의 헬퍼로 교체하세요.",
                "kcfix-updater.exe is not compatible with this program. Replace it with the one from the release zip.",
                "kcfix-updater.exe ヘルパーはこのプログラムと互換性がありません。リリースzipのヘルパーに置き換えてください。",
            ),
            Self::HelperMissing => (
                "kcfix-updater.exe 헬퍼를 찾을 수 없습니다. 앱과 같은 폴더에 함께 두세요.",
                "kcfix-updater.exe was not found. Keep it in the same folder as the program.",
                "kcfix-updater.exe ヘルパーが見つかりません。プログラムと同じフォルダーに入れてください。",
            ),
            Self::HelperStage => (
                "업데이트 헬퍼를 준비하지 못했습니다.",
                "Could not prepare the update helper.",
                "更新ヘルパーを準備できませんでした。",
            ),
            Self::HelperLaunch => (
                "업데이트 헬퍼를 실행하지 못했습니다.",
                "Could not start the update helper.",
                "更新ヘルパーを起動できませんでした。",
            ),
            Self::Encoding => (
                "업데이트 데이터를 처리하지 못했습니다.",
                "Could not process the update data.",
                "更新データを処理できませんでした。",
            ),
        }
    }

    pub fn text(self, lang: Language) -> &'static str {
        let (ko, en, ja) = self.texts();
        match lang {
            Language::Ko => ko,
            Language::En => en,
            Language::Ja => ja,
        }
    }
}

#[derive(Debug)]
pub enum UpdateError {
    /// A failure; `detail` holds the underlying technical error, if any.
    Failed {
        kind: UpdateErrorKind,
        detail: Option<String>,
    },
    NoUpdate,
}

impl UpdateError {
    pub fn new(kind: UpdateErrorKind) -> Self {
        Self::Failed { kind, detail: None }
    }

    pub fn with_detail(kind: UpdateErrorKind, detail: impl std::fmt::Display) -> Self {
        Self::Failed {
            kind,
            detail: Some(detail.to_string()),
        }
    }

    /// Error kind, `None` for `NoUpdate`.
    #[cfg(test)]
    pub fn kind(&self) -> Option<UpdateErrorKind> {
        match self {
            Self::Failed { kind, .. } => Some(*kind),
            Self::NoUpdate => None,
        }
    }

    /// User-facing text in `lang`, with the technical detail appended.
    pub fn message(&self, lang: Language) -> String {
        match self {
            Self::Failed { kind, detail } => match detail {
                Some(detail) => format!("{}\n({detail})", kind.text(lang)),
                None => kind.text(lang).to_string(),
            },
            Self::NoUpdate => match lang {
                Language::Ko => "현재 최신 버전을 사용 중입니다.".to_string(),
                Language::En => "You are using the latest version.".to_string(),
                Language::Ja => "最新バージョンをお使いです。".to_string(),
            },
        }
    }
}

/// English, for logs and the terminal; the UI uses [`UpdateError::message`].
impl std::fmt::Display for UpdateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message(Language::En))
    }
}

impl std::error::Error for UpdateError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_has_distinct_text_in_each_language() {
        for kind in UpdateErrorKind::ALL {
            let texts = [
                kind.text(Language::Ko),
                kind.text(Language::En),
                kind.text(Language::Ja),
            ];
            assert!(texts.iter().all(|t| !t.is_empty()), "{kind:?}");
            assert_ne!(texts[0], texts[1], "{kind:?}");
            assert_ne!(texts[0], texts[2], "{kind:?}");
            assert_ne!(texts[1], texts[2], "{kind:?}");
        }
        assert_ne!(
            UpdateError::NoUpdate.message(Language::Ja),
            UpdateError::NoUpdate.message(Language::En)
        );
    }

    #[test]
    fn message_appends_detail_in_every_language() {
        let err = UpdateError::with_detail(UpdateErrorKind::Download, "timeout");
        assert_eq!(
            err.message(Language::En),
            "Could not download the update.\n(timeout)"
        );
        assert!(err.message(Language::Ko).ends_with("(timeout)"));
        assert!(err.message(Language::Ja).ends_with("(timeout)"));
        assert_eq!(
            err.message(Language::Ja),
            "更新ファイルをダウンロードできませんでした。\n(timeout)"
        );
        assert_eq!(err.kind(), Some(UpdateErrorKind::Download));
        assert_eq!(UpdateError::NoUpdate.kind(), None);
    }
}
