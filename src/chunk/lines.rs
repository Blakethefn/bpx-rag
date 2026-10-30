
pub(super) fn truncate_title(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let prefix = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{prefix}...")
    } else {
        prefix
    }
}

pub(super) fn line_start_offsets(content: &str) -> Vec<usize> {
    let mut offsets = Vec::with_capacity(content.bytes().filter(|byte| *byte == b'\n').count() + 1);
    offsets.push(0);
    offsets.extend(
        content
            .bytes()
            .enumerate()
            .filter_map(|(index, byte)| (byte == b'\n').then_some(index + 1)),
    );
    offsets
}

pub(super) fn line_for_byte(line_starts: &[usize], byte: usize) -> u32 {
    line_starts.partition_point(|start| *start <= byte).max(1) as u32
}
