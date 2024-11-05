use byteorder::{ByteOrder, LittleEndian};

const ENVP_STACK_TOP: usize = 0;
const ENVP_IPC_BUFFER: usize = 1;
const ENVP_RS_SHARED_BUF: usize = 2;
const ENVP_MIN_SIZE: usize = 3;

#[derive(Debug, Copy, Clone)]
struct EnvVarsInner {
    inner: [u64; 32],
    size: usize,
    index: usize,
}

pub struct EnvVars {
    inner: EnvVarsInner,
}

#[allow(non_upper_case_globals)]
static mut env_inner: Option<EnvVarsInner> = None;

pub unsafe fn init_env(envp: *const u8) {
    let mut env_inner_tmp = EnvVarsInner {
        inner: [0; 32], // @alwin: HACK,
        size: 0,
        index: 0,
    };

    // @alwin: HACK
    for i in 0..32 {
        LittleEndian::read_u64_into(
            core::slice::from_raw_parts(envp.add(i * 8), 8),
            &mut env_inner_tmp.inner[i..i + 1],
        );
        if env_inner_tmp.inner[i] == 0 {
            env_inner_tmp.size = i;
            break;
        }
    }

    if env_inner_tmp.size < ENVP_MIN_SIZE {
        panic!("Recieved a corrupted envp");
    }

    env_inner = Some(env_inner_tmp);
}

pub fn env_vars() -> EnvVars {
    EnvVars {
        inner: unsafe { env_inner.unwrap() },
    }
}

pub fn stack_top() -> usize {
    unsafe { env_inner.unwrap().nth(ENVP_STACK_TOP).unwrap() as usize }
}

pub fn rs_shared_buf() -> usize {
    unsafe { env_inner.unwrap().nth(ENVP_RS_SHARED_BUF).unwrap() as usize }
}

pub fn ipc_buffer() -> usize {
    unsafe { env_inner.unwrap().nth(ENVP_IPC_BUFFER).unwrap() as usize }
}

impl Iterator for EnvVars {
    // @alwin: This should maybe eventually be strings
    type Item = u64;

    fn next(&mut self) -> Option<u64> {
        self.inner.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl Iterator for EnvVarsInner {
    // @alwin: This should maybe eventually be strings
    type Item = u64;

    fn next(&mut self) -> Option<u64> {
        if self.index < self.size {
            let res = unsafe { *(self.inner[self.index] as *const u64) };
            self.index += 1;
            Some(res)
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.size - self.index, Some(self.size - self.index))
    }
}
