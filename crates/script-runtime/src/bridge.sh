# bashdb owns stepping and breakpoints; this bridge only transports its state.
_Dbg_tundra_original_flags=$-
set +e +u
if (( BASH_VERSINFO[0] < 5 || (BASH_VERSINFO[0] == 5 && BASH_VERSINFO[1] < 2) )); then
    printf '%s\n' 'bashdb requires Bash 5.2 or newer' >&2
    exit 2
fi
_Dbg_tundra_root=$TUNDRA_SCRIPT_DEBUG_ROOT
unset BASH_ENV TUNDRA_SCRIPT_DEBUG_ROOT
source "$_Dbg_tundra_root/bashdb-trace" -n -q --no-highlight -L "$_Dbg_tundra_root" --tempdir "$_Dbg_tundra_root" --tty "$_Dbg_tundra_root/control.out" --tty_in "$_Dbg_tundra_root/control.in"
_Dbg_tundra_encode() { printf '%s' "$1" | base64 -w0; }
_Dbg_tundra_emit() { printf '\n__TUNDRA__\t%s\n' "$*" >> "$_Dbg_tty"; }
_Dbg_tundra_stop() {
    _Dbg_tundra_emit "STOP $(_Dbg_tundra_encode "$_Dbg_frame_last_filename") $_Dbg_frame_last_lineno"
}
_Dbg_tundra_command() {
    local _Dbg_tundra_op=$1 _Dbg_tundra_data=${2:-} _Dbg_tundra_id=${3:-0}
    case $_Dbg_tundra_op in
        inspect)
            _Dbg_tundra_emit "VARS $(_Dbg_tundra_encode "$(declare -p)")"
            local _Dbg_tundra_i
            for ((_Dbg_tundra_i=1; _Dbg_tundra_i<${#FUNCNAME[@]}; _Dbg_tundra_i++)); do
                [[ ${FUNCNAME[_Dbg_tundra_i]} == _Dbg_* ]] && continue
                [[ ${BASH_SOURCE[_Dbg_tundra_i]} == "$_Dbg_tundra_root"/* ]] && continue
                _Dbg_tundra_emit "FRAME $(_Dbg_tundra_encode "${FUNCNAME[_Dbg_tundra_i]}") $(_Dbg_tundra_encode "${BASH_SOURCE[_Dbg_tundra_i]}") ${BASH_LINENO[_Dbg_tundra_i-1]}"
            done
            _Dbg_tundra_emit 'INSPECTED'
            ;;
        clear) _Dbg_clear_all_brkpt ;;
        bp)
            local _Dbg_tundra_path _Dbg_tundra_before=$_Dbg_brkpt_count
            _Dbg_tundra_path=$(printf '%s' "$_Dbg_tundra_data" | base64 -d; printf '.')
            _Dbg_tundra_path=${_Dbg_tundra_path%.}
            _Dbg_readin_if_new "$_Dbg_tundra_path"
            _Dbg_get_source_line "$_Dbg_tundra_id" "$_Dbg_tundra_path"
            if [[ $_Dbg_source_line =~ ^[[:space:]]*($|#) ]] ||
               [[ $_Dbg_source_line =~ ^[[:space:]]*(fi|done|esac|\})[[:space:]]*(#.*)?$ ]] ||
               [[ $_Dbg_source_line =~ ^[[:space:]]*(function[[:space:]]+)?[[:alnum:]_]+[[:space:]]*(\([[:space:]]*\))?[[:space:]]*\{[[:space:]]*(#.*)?$ ]]; then
                _Dbg_tundra_emit "BP $_Dbg_tundra_data $_Dbg_tundra_id 0"
            elif _Dbg_check_line "$_Dbg_tundra_id" "$_Dbg_tundra_path"; then
                _Dbg_set_brkpt "$_Dbg_tundra_path" "$_Dbg_tundra_id" 0 1
                _Dbg_tundra_emit "BP $_Dbg_tundra_data $_Dbg_tundra_id 1"
            else
                _Dbg_tundra_emit "BP $_Dbg_tundra_data $_Dbg_tundra_id 0"
            fi
            ;;
        eval|examine)
            local _Dbg_tundra_expr
            _Dbg_tundra_expr=$(printf '%s' "$_Dbg_tundra_data" | base64 -d; printf '.')
            _Dbg_tundra_expr=${_Dbg_tundra_expr%.}
            _Dbg_tundra_emit "EVAL_BEGIN $_Dbg_tundra_id"
            if [[ $_Dbg_tundra_op == eval ]]; then
                _Dbg_do_eval "$_Dbg_tundra_expr" >> "$_Dbg_tty" 2>&1
            else
                _Dbg_do_examine "$_Dbg_tundra_expr" >> "$_Dbg_tty" 2>&1
            fi
            _Dbg_tundra_emit "EVAL_END $_Dbg_tundra_id"
            ;;
    esac
    return 0
}
_Dbg_debugger_commands[tundra]=_Dbg_tundra_command
_Dbg_cmdloop_hooks[tundra]=_Dbg_tundra_stop
_Dbg_QUIT_ON_QUIT=1
_Dbg_set_annotate=0
_Dbg_set_show_command=0
_Dbg_set_trace_init=
if [[ $_Dbg_tundra_original_flags == *u* ]]; then set -u; fi
if [[ $_Dbg_tundra_original_flags == *e* ]]; then set -e; fi
_Dbg_set_trace
