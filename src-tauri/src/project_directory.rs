#[tauri::command]
pub async fn choose_project_directory(
    window: tauri::WebviewWindow,
) -> Result<Option<String>, String> {
    #[cfg(windows)]
    {
        let owner = window.hwnd().map_err(|error| error.to_string())?.0 as isize;
        tauri::async_runtime::spawn_blocking(move || windows::choose(owner))
            .await
            .map_err(|error| error.to_string())?
    }
    #[cfg(not(windows))]
    {
        let _ = window;
        Err("当前平台请手动输入项目目录".into())
    }
}

#[cfg(windows)]
mod windows {
    use std::ffi::c_void;
    use std::ptr::{null, null_mut};

    #[repr(C)]
    struct BrowseInfo {
        owner: *mut c_void,
        root: *const c_void,
        display_name: *mut u16,
        title: *const u16,
        flags: u32,
        callback: Option<unsafe extern "system" fn(*mut c_void, u32, isize, isize) -> i32>,
        parameter: isize,
        image: i32,
    }

    #[link(name = "shell32")]
    unsafe extern "system" {
        fn SHBrowseForFolderW(info: *const BrowseInfo) -> *mut c_void;
        fn SHGetPathFromIDListEx(
            item: *const c_void,
            path: *mut u16,
            length: u32,
            flags: u32,
        ) -> i32;
    }
    #[link(name = "ole32")]
    unsafe extern "system" {
        fn CoInitializeEx(reserved: *mut c_void, flags: u32) -> i32;
        fn CoUninitialize();
        fn CoTaskMemFree(memory: *mut c_void);
    }

    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe { CoUninitialize() }
        }
    }

    struct Item(*mut c_void);
    impl Drop for Item {
        fn drop(&mut self) {
            unsafe { CoTaskMemFree(self.0) }
        }
    }

    pub fn choose(owner: isize) -> Result<Option<String>, String> {
        // 在阻塞工作线程初始化 STA；不阻塞 Tauri 主线程，也不引入下载依赖。
        let status = unsafe { CoInitializeEx(null_mut(), 2) };
        if status < 0 {
            return Err(format!("无法打开文件夹选择器：{status:#x}"));
        }
        let _apartment = Apartment;
        let title = "选择项目文件夹\0".encode_utf16().collect::<Vec<_>>();
        let mut display_name = vec![0_u16; 260];
        let info = BrowseInfo {
            owner: owner as *mut c_void,
            root: null(),
            display_name: display_name.as_mut_ptr(),
            title: title.as_ptr(),
            flags: 0x51, // RETURNONLYFSDIRS | EDITBOX | NEWDIALOGSTYLE
            callback: None,
            parameter: 0,
            image: 0,
        };
        let item = unsafe { SHBrowseForFolderW(&info) };
        if item.is_null() {
            return Ok(None);
        }
        let item = Item(item);
        let mut path = vec![0_u16; 32768];
        if unsafe { SHGetPathFromIDListEx(item.0, path.as_mut_ptr(), path.len() as u32, 0) } == 0 {
            return Err("请选择本地文件系统中的文件夹".into());
        }
        let length = path
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(path.len());
        String::from_utf16(&path[..length])
            .map(Some)
            .map_err(|_| "项目目录包含无法表示的字符".into())
    }
}
