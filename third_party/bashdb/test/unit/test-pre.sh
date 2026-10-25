#!/src/build/bash-5.1/bash
# -*- shell-script -*-

do_test() 
{
    file1=$(_Dbg_expand_filename $1)
    assertTrue 'Should not have an error expanding $1' "$?"
    file2=$(_Dbg_expand_filename $2)
    assertEquals "$file1" "$file2"

}
test_pre_expand_filename()
{
    do_test test-pre ./test-pre
    do_test "$PWD/" "${PWD}/."
    do_test /tmp /tmp
    file1=$(echo ~)
    do_test "$file1" ~
}

test_pre_do_show_version()
{
    # Name we refer to ourselves by
    typeset _Dbg_debugger_name='bashdb'
    # The release name we are configured to run under.
    typeset _Dbg_release='5.2-1.2.0'

    version_string=$(_Dbg_do_show_version 2>&1)
    assertEquals "bashdb, release 5.2-1.2.0" "$version_string"
}
abs_top_srcdir=/src/external-vcs/github/Trepan-Debuggers/bashdb
# Make sure $abs_top_srcdir has a trailing slash
abs_top_srcdir=${abs_top_srcdir%%/}/
. ${abs_top_srcdir}test/unit/helper.sh
. ${abs_top_srcdir}/init/pre.sh
set -- # reset $# so shunit2 doesn't get confused.

[[ $0 == ${BASH_SOURCE} ]] && . ${shunit_file}

