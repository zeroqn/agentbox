//! Guest process argv and working-directory contribution.

use super::super::model::WORKSPACE_TARGET;

pub(crate) fn workdir_from_image(working_dir: Option<&str>) -> String {
    working_dir
        .filter(|working_dir| !working_dir.is_empty())
        .unwrap_or(WORKSPACE_TARGET)
        .to_owned()
}

pub(crate) fn guest_init_argv(guest_command: &[String], image_cmd: &[String]) -> Vec<String> {
    let command = if guest_command.is_empty() {
        if image_cmd.is_empty() {
            vec!["fish".to_owned(), "-l".to_owned()]
        } else {
            image_cmd.to_vec()
        }
    } else {
        guest_command.to_vec()
    };

    std::iter::once("enter".to_owned()).chain(command).collect()
}
