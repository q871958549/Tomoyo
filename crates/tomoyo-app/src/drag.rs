//! Native Windows OLE File Drag-Out (`DoDragDrop`) allowing users to drag any Everything
//! file or folder result directly into Explorer, VS Code, WeChat, or browser upload dialogs.

use std::path::Path;

/// Starts a synchronous Windows OLE file drag operation for `file_path`.
#[cfg(windows)]
pub fn start_file_drag(file_path: &str) -> bool {
    use windows::core::{implement, Error, PCWSTR, Result, HRESULT};
    use windows::Win32::Foundation::{DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, HWND, S_OK};
    use windows::Win32::System::Com::IDataObject;
    use windows::Win32::System::Ole::{
        DoDragDrop, IDropSource, IDropSource_Impl, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_LINK,
    };
    use windows::Win32::System::SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS};
    use windows::Win32::UI::Shell::{IShellFolder, SHBindToParent, SHParseDisplayName};

    if !Path::new(file_path).exists() {
        return false;
    }

    #[implement(IDropSource)]
    struct SimpleDropSource;

    impl IDropSource_Impl for SimpleDropSource_Impl {
        fn QueryContinueDrag(
            &self,
            fescapepressed: windows::core::BOOL,
            grfkeystate: MODIFIERKEYS_FLAGS,
        ) -> HRESULT {
            if fescapepressed.as_bool() {
                return DRAGDROP_S_CANCEL;
            }
            if (grfkeystate.0 & MK_LBUTTON.0) == 0 {
                return DRAGDROP_S_DROP;
            }
            S_OK
        }

        fn GiveFeedback(&self, _dweffect: DROPEFFECT) -> HRESULT {
            // DRAGDROP_S_USEDEFAULTCURSORS (0x00040102)
            HRESULT(0x0004_0102)
        }
    }

    let wide: Vec<u16> = file_path.encode_utf16().chain(std::iter::once(0)).collect();

    unsafe {
        let mut pidl = std::ptr::null_mut();
        if SHParseDisplayName(PCWSTR(wide.as_ptr()), None, &mut pidl, 0, None).is_err()
            || pidl.is_null()
        {
            return false;
        }

        let drag_res: Result<()> = (|| {
            let mut child_pidl = std::ptr::null_mut();
            let folder: IShellFolder = SHBindToParent(pidl, Some(&mut child_pidl))?;
            let child_arr = [child_pidl as *const _];
            let data_obj: IDataObject = folder.GetUIObjectOf(HWND::default(), &child_arr, None)?;
            let drop_source: IDropSource = SimpleDropSource.into();
            let mut effect = DROPEFFECT(0);
            let hr = DoDragDrop(
                &data_obj,
                &drop_source,
                DROPEFFECT_COPY | DROPEFFECT_LINK,
                &mut effect,
            );
            if hr == DRAGDROP_S_DROP || hr == DRAGDROP_S_CANCEL || hr == S_OK {
                Ok(())
            } else {
                Err(Error::from(hr))
            }
        })();

        windows::Win32::System::Com::CoTaskMemFree(Some(pidl as *const _));
        drag_res.is_ok()
    }
}

#[cfg(not(windows))]
pub fn start_file_drag(_file_path: &str) -> bool {
    false
}
