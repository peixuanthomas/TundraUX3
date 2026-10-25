" The transport carries buffers in memory; this script contains no document text.
if !has('eval') || !has('channel') || !has('timers') || !exists('*str2blob') || !exists('*listener_add')
  echoerr 'Tundra requires Vim with +eval, +channel, +timers and str2blob()'
  cquit 78
endif

let s:revision = 0
let s:last_command = 0
let s:save_id = 0
let s:responses = {}
let s:text_versions = {}
let s:removed = []
let s:exiting = v:false
let s:error = v:null
let s:mode = 'n'
let s:busy = 0
let s:writing = 0
let s:importing = 0
let s:deferred = []
let s:initialized = 0
let s:last_state = ''

function! s:Send(message) abort
  call ch_sendraw(s:channel, json_encode(a:message) . "\n")
endfunction

function! s:Record(info, force) abort
  let n = a:info.bufnr
  call s:Watch(n)
  call listener_flush(n)
  call filter(s:removed, {_, value -> value != n})
  let path = bufname(n)
  let cursor = n == bufnr('%') ? [line('.') - 1, col('.') - 1] : getbufvar(n, 'tundra_cursor', [0, 0])
  let record = {'number': n, 'host_id': getbufvar(n, 'tundra_host_id', v:null),
        \ 'path': empty(path) ? v:null : fnamemodify(path, ':p'),
        \ 'encoding': empty(getbufvar(n, '&fileencoding')) ? 'utf-8' : getbufvar(n, '&fileencoding'),
        \ 'line_ending': getbufvar(n, '&fileformat'),
        \ 'bom': getbufvar(n, '&bomb') ? v:true : v:false,
        \ 'end_of_line': getbufvar(n, '&endofline') ? v:true : v:false,
        \ 'modified': getbufvar(n, '&modified') ? v:true : v:false,
        \ 'cursor': {'line': cursor[0], 'column': cursor[1]},
        \ 'read_only': getbufvar(n, 'tundra_read_only', 0) ? v:true : v:false,
        \ 'protected': getbufvar(n, 'tundra_protected', 0) ? v:true : v:false,
        \ 'changed_tick': a:info.changedtick,
        \ 'line_changes': getbufvar(n, 'tundra_line_changes', [])}
  call setbufvar(n, 'tundra_line_changes', [])
  let content_version = string([a:info.changedtick, record.line_ending, record.end_of_line])
  if a:force || get(s:text_versions, string(n), '') !=# content_version
    let truly_empty = n == bufnr('%') ? (line('$') == 1 && getline(1) ==# '' && wordcount().bytes == 0)
          \ : getbufvar(n, 'tundra_empty', 0)
    let record.text_lines = truly_empty ? [] : getbufline(n, 1, '$')
    call setbufvar(n, 'tundra_empty', truly_empty)
    let record.text_changed = v:true
    let s:text_versions[string(n)] = content_version
  endif
  return record
endfunction

function! s:Snapshot(force) abort
  if !exists('s:channel') || ch_status(s:channel) !=# 'open'
    return
  endif
  let s:revision += 1
  let buffers = []
  let dirty = 0
  for info in getbufinfo({'bufloaded': 1})
    if !s:initialized || !info.listed || getbufvar(info.bufnr, '&buftype') !~# '^\%(\|acwrite\)$'
      continue
    endif
    call add(buffers, s:Record(info, a:force))
    let dirty = dirty || info.changed
  endfor
  let state = {'kind': 'state', 'ready': s:initialized ? v:true : v:false,
        \ 'last_command': s:last_command, 'active_buffer': bufnr('%'),
        \ 'mode': s:mode, 'buffers': buffers, 'removed_buffers': s:removed,
        \ 'exiting': s:exiting, 'discard_modified_on_exit': s:exiting && dirty ? v:true : v:false,
        \ 'error': s:error}
  let signature = json_encode(state)
  if a:force || signature !=# s:last_state
    let s:last_state = signature
    let state.revision = s:revision
    call s:Send(state)
  endif
endfunction

function! s:LinesChanged(buffer, start, end, added, changes) abort
  if s:importing
    return
  endif
  let pending = getbufvar(a:buffer, 'tundra_line_changes', [])
  for change in a:changes
    call add(pending, {'start_line': change.lnum, 'end_line': change.end,
          \ 'added': change.added, 'column': change.col})
  endfor
  call setbufvar(a:buffer, 'tundra_line_changes', pending)
endfunction

function! s:Watch(buffer) abort
  if !getbufvar(a:buffer, 'tundra_listener', 0)
    call setbufvar(a:buffer, 'tundra_listener', listener_add(function('s:LinesChanged'), a:buffer))
  endif
endfunction

function! s:Protect() abort
  if &l:buftype !=# '' && &l:buftype !=# 'acwrite'
    return
  endif
  setlocal buftype=acwrite noswapfile noundofile
  call s:Watch(bufnr('%'))
  if get(b:, 'tundra_read_only', 0)
    setlocal readonly nomodifiable
  endif
endfunction

function! s:Open(buffer) abort
  let s:importing += 1
  try
  let input = a:buffer
  let found = -1
  for info in getbufinfo()
    if getbufvar(info.bufnr, 'tundra_host_id', -1) == input.host_id
      let found = info.bufnr
      break
    endif
  endfor
  if found > 0
    execute 'keepalt hide buffer ' . found
  elseif empty(bufname('%')) && !&modified && !exists('b:tundra_host_id')
    " Reuse Vim's initial unnamed buffer without making an extra document.
  else
    keepalt hide enew
  endif
  let s:initialized = 1
  let b:tundra_host_id = input.host_id
  let b:tundra_read_only = input.read_only
  let b:tundra_protected = input.protected
  let unchanged = found > 0 && getbufline(bufnr('%'), 1, '$') ==# input.lines
        \ && get(b:, 'tundra_empty', 0) == input.empty
  setlocal modifiable noreadonly
  if !unchanged
    let saved_undo_levels = &l:undolevels
    if found < 0
      setlocal undolevels=-1
    endif
    let lines = input.lines
    if empty(lines)
      let lines = ['']
    endif
    if input.empty
      if found > 0
        silent keepjumps %delete _
      endif
    else
      call setline(1, lines)
      if line('$') > len(lines)
        silent! undojoin
        call deletebufline(bufnr('%'), len(lines) + 1, '$')
      endif
    endif
    if found < 0
      let &l:undolevels = saved_undo_levels
    endif
  endif
  if type(input.path) == v:t_string && !empty(input.path) && expand('%:p') !=# input.path
    execute 'silent keepalt file ' . fnameescape(input.path)
  endif
  let b:tundra_read_path = type(input.path) == v:t_string && !empty(input.path)
        \ ? fnamemodify(input.path, ':p') : ''
  let &l:fileencoding = input.encoding
  let &l:fileformat = input.line_ending
  let &l:bomb = input.bom
  let &l:endofline = input.end_of_line
  let &l:modified = input.modified
  let b:tundra_empty = input.empty
  call cursor(input.cursor.line + 1, input.cursor.column + 1)
  call s:Protect()
  call listener_flush(bufnr('%'))
  call s:Snapshot(1)
  finally
    let s:importing -= 1
  endtry
endfunction

function! s:Activate(message) abort
  let number = get(a:message, 'number', -1)
  if number < 0
    for info in getbufinfo()
      if getbufvar(info.bufnr, 'tundra_host_id', -1) == a:message.host_id
        let number = info.bufnr
        break
      endif
    endfor
  endif
  if number > 0 && bufexists(number)
    execute 'keepalt hide buffer ' . number
  else
    throw 'Tundra: buffer is no longer available'
  endif
endfunction

function! s:Receive(channel, raw) abort
  let message = json_decode(a:raw)
  if get(message, 'op', '') ==# 'save_response'
    let s:responses[string(message.request)] = message
    return
  endif
  if s:writing
    call add(s:deferred, message)
    return
  endif
  call s:Dispatch(message)
endfunction

function! s:Dispatch(message) abort
  let message = a:message
  let s:busy += 1
  let s:last_command = max([s:last_command, get(message, 'id', 0)])
  let s:error = v:null
  try
    if message.op ==# 'open'
      call s:Open(message.buffer)
    elseif message.op ==# 'activate'
      call s:Activate(message)
    elseif message.op ==# 'bind'
      call setbufvar(message.number, 'tundra_host_id', message.host_id)
    elseif message.op ==# 'remove'
      execute 'silent! bwipeout! ' . message.number
    elseif message.op ==# 'policy'
      call setbufvar(message.number, 'tundra_read_only', message.read_only)
      call setbufvar(message.number, 'tundra_protected', message.protected)
      call setbufvar(message.number, '&readonly', message.read_only)
      call setbufvar(message.number, '&modifiable', !message.read_only)
    elseif message.op ==# 'close'
      execute message.force ? 'qall!' : 'qall'
    endif
  catch
    let s:error = v:exception
    echohl ErrorMsg
    echomsg v:exception
    echohl None
  finally
    let s:busy -= 1
    call s:Snapshot(1)
    redraw
  endtry
endfunction

function! s:Bytes(lines, encoding, fileformat, final_newline, bom) abort
  let result = 0z
  let encoding = tolower(substitute(a:encoding, '_', '-', 'g'))
  if encoding =~# '^utf-\?16$'
    let encoding = 'utf-16be'
  elseif encoding =~# '^utf-\?32$'
    let encoding = 'utf-32be'
  elseif encoding ==# 'ucs-2'
    let encoding = 'ucs-2be'
  elseif encoding ==# 'ucs-4'
    let encoding = 'ucs-4be'
  endif
  let options = {'encoding': encoding}
  if a:bom && a:encoding =~? '^\%(utf-\?8\|utf-16\|utf-32\|ucs-2\|ucs-4\)'
    let result += str2blob(["\ufeff"], options)
  endif
  let ending = a:fileformat ==# 'dos' ? str2blob(["\r", ''], options)
        \ : (a:fileformat ==# 'mac' ? str2blob(["\r"], options) : str2blob(['', ''], options))
  for index in range(len(a:lines))
    let result += str2blob([a:lines[index]], options)
    if index < len(a:lines) - 1 || a:final_newline
      let result += ending
    endif
  endfor
  return blob2list(result)
endfunction

function! s:Write(path, whole, append) abort
  let s:writing += 1
  try
  if get(b:, 'tundra_read_only', 0)
    throw 'Tundra: this buffer is read-only'
  endif
  let s:save_id += 1
  let request = s:save_id
  let number = bufnr('%')
  let tick = b:changedtick
  let target = empty(a:path) ? expand('%:p') : fnamemodify(a:path, ':p')
  if &l:readonly && !v:cmdbang
    throw 'Tundra: E45: readonly option is set (add ! to override)'
  endif
  if !v:cmdbang && !a:append && target !=# get(b:, 'tundra_read_path', '') && getftype(target) !=# ''
    throw 'Tundra: E13: File exists (add ! to override)'
  endif
  if !v:cmdbang && !a:whole && !a:append && target ==# expand('%:p')
    throw 'Tundra: E140: Use ! to write partial buffer'
  endif
  let binary = &l:binary
  if v:cmdarg =~# '++bin\%[ary]\>'
    let binary = 1
  elseif v:cmdarg =~# '++nobin\%[ary]\>'
    let binary = 0
  endif
  let lines = a:whole ? getline(1, '$') : getline("'[", "']")
  let final_newline = (a:whole || line("']") == line('$'))
        \ ? (&l:endofline || (!binary && &l:fixendofline)) : 1
  if a:whole && len(lines) == 1 && lines[0] ==# '' && wordcount().bytes == 0
    let lines = []
  endif
  let encoding = matchstr(v:cmdarg, '++enc\%[oding]=\zs\S\+')
  let encoding = empty(encoding) ? (empty(&l:fileencoding) ? 'utf-8' : &l:fileencoding) : encoding
  let fileformat = matchstr(v:cmdarg, '++ff\%[ileformat]=\zs\S\+')
  let fileformat = empty(fileformat) ? (binary ? 'unix' : &l:fileformat) : fileformat
  let ending = fileformat ==# 'dos' ? "\r\n" : (fileformat ==# 'mac' ? "\r" : "\n")
  let output = join(lines, ending) . (final_newline && !empty(lines) ? ending : '')
  let bom = &l:bomb && !a:append && !binary
  let bytes = s:Bytes(lines, encoding, fileformat, final_newline, bom)
  call s:Snapshot(1)
  call s:Send({'kind': 'save', 'id': request, 'buffer_number': number,
        \ 'host_id': get(b:, 'tundra_host_id', v:null), 'path': empty(target) ? v:null : target,
        \ 'text': output, 'text_lines': lines, 'bytes': bytes, 'encoding': encoding,
        \ 'line_ending': fileformat, 'bom': bom ? v:true : v:false,
        \ 'end_of_line': final_newline ? v:true : v:false,
        \ 'changed_tick': tick, 'protected': get(b:, 'tundra_protected', 0) ? v:true : v:false,
        \ 'current_path': empty(bufname('%')) ? v:null : expand('%:p'),
        \ 'whole_buffer': a:whole ? v:true : v:false, 'append': a:append ? v:true : v:false,
        \ 'force': v:cmdbang ? v:true : v:false})
  while !has_key(s:responses, string(request))
    if ch_status(s:channel) !=# 'open'
      throw 'Tundra: editor connection closed before save completed'
    endif
    sleep 10m
  endwhile
  let response = remove(s:responses, string(request))
  if !response.ok
    throw 'Tundra: ' . get(response, 'error', 'save failed')
  endif
  if bufnr('%') == number && target ==# expand('%:p')
    let b:tundra_read_path = target
  endif
  if a:whole && !a:append && bufnr('%') == number && b:changedtick == tick
        \ && (target ==# expand('%:p') || stridx(&cpoptions, '+') < 0)
    let &l:endofline = final_newline
    setlocal nomodified
  endif
  call s:Snapshot(1)
  redraw
  finally
    let s:writing -= 1
  endtry
endfunction

function! s:Changed() abort
  let s:mode = mode(1)
  let b:tundra_cursor = [line('.') - 1, col('.') - 1]
  call s:Snapshot(0)
endfunction

function! s:Removed(number) abort
  let number = str2nr(a:number)
  call add(s:removed, number)
  if has_key(s:text_versions, string(number))
    call remove(s:text_versions, string(number))
  endif
endfunction

function! s:Leaving() abort
  let s:exiting = v:true
  call s:Snapshot(1)
endfunction

function! s:Tick(timer) abort
  if !s:busy && !s:writing
    while !empty(s:deferred)
      call s:Dispatch(remove(s:deferred, 0))
    endwhile
    call s:Changed()
  endif
endfunction

set encoding=utf-8 showmode showcmd ruler laststatus=2 mouse=a
set nobackup nowritebackup
let s:channel = ch_open($TUNDRA_VIM_ENDPOINT, {'mode': 'nl', 'callback': function('s:Receive')})
if ch_status(s:channel) !=# 'open'
  echoerr 'Tundra: could not connect to the editor host'
  cquit 79
endif
call s:Send({'kind': 'hello', 'token': $TUNDRA_VIM_TOKEN})
unlet $TUNDRA_VIM_TOKEN
augroup TundraVimBridge
  autocmd!
  autocmd BufEnter * call s:Protect()
  autocmd BufReadPost * let b:tundra_read_path = expand('%:p')
  autocmd BufWriteCmd * call s:Write(expand('<afile>'), 1, 0)
  autocmd FileWriteCmd * call s:Write(expand('<afile>'), 0, 0)
  autocmd FileAppendCmd * call s:Write(expand('<afile>'), 0, 1)
  autocmd BufDelete * call s:Removed(expand('<abuf>'))
  autocmd VimLeavePre * call s:Leaving()
  autocmd TextChanged,TextChangedI,CursorMoved,CursorMovedI,ModeChanged * call s:Changed()
augroup END
call s:Protect()
call s:Snapshot(1)
call timer_start(50, function('s:Tick'), {'repeat': -1})
