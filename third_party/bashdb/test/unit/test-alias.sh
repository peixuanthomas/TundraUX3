#!/src/build/bash-5.1/bash
# -*- shell-script -*-
test_alias()
{
    _Dbg_alias_add u up
    typeset _Dbg_expanded_alias=''; _Dbg_alias_expand u
    assertEquals 'up' $_Dbg_expanded_alias

    _Dbg_alias_add q quit
    _Dbg_expanded_alias=''; _Dbg_alias_expand q
    assertEquals 'quit' $_Dbg_expanded_alias

    typeset _Dbg_aliases_found=''
    _Dbg_alias_find_aliased quit
    assertEquals 'q' "$_Dbg_aliases_found"

    _Dbg_alias_add exit quit
    _Dbg_alias_find_aliased quit
    assertEquals 'exit, q' "$_Dbg_aliases_found"

    _Dbg_alias_remove q
    _Dbg_expanded_alias=''; _Dbg_alias_expand q
    assertEquals 'q' $_Dbg_expanded_alias

    _Dbg_alias_find_aliased quit
    assertEquals 'exit' "$_Dbg_aliases_found"

    _Dbg_expanded_alias=''; _Dbg_alias_expand u
    assertEquals 'up' $_Dbg_expanded_alias
}

if [ '/src/external-vcs/github/Trepan-Debuggers/bashdb' = '' ] ; then
  echo "Something is wrong: abs_top_srcdir is not set."
 exit 1
fi
abs_top_srcdir=/src/external-vcs/github/Trepan-Debuggers/bashdb
# Make sure $abs_top_src has a trailing slash
abs_top_srcdir=${abs_top_srcdir%%/}/
. ${abs_top_srcdir}test/unit/helper.sh
. $abs_top_srcdir/lib/help.sh
. $abs_top_srcdir/lib/alias.sh
. $abs_top_srcdir/lib/sort.sh
set -- # reset $# so shunit2 doesn't get confused.
[[ $0 == ${BASH_SOURCE} ]] && . ${shunit_file}
