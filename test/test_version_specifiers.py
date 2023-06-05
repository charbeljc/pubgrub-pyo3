import pytest
from pubgrub import VersionSpecifiers

@pytest.mark.parametrize(
    "input,expected",
    [
        # https://peps.python.org/pep-0440/#examples
        ("~=3.1", "3.1 <= v < 4"),
        ("~=3.1.2", "3.1.2 <= v < 3.2"),
        ("~=3.1a1", "3.1a1 <= v < 4"),
        ("==3.1", "3.1"),
        ("==2.*", "2 <= v < 3"),
        ("~=2.0", "2.0 <= v < 3"),
        ("==3.1.*", "3.1 <= v < 3.2"),
        ("~=3.1.0", "3.1.0 <= v < 3.2"),
        ("~=3.1.0, != 3.1.3", "[ 3.1.0, 3.1.3 [  [ 3.1.3.post0.dev0, 3.2 ["),
    ]
)
def test_version_specifier_to_pubgrub(input, expected):
    vs = VersionSpecifiers(input)
    rg = vs.to_pubgrub()
    assert str(rg) == expected

@pytest.mark.parametrize(
    "left,right",
    [
        # https://peps.python.org/pep-0440/#compatible-release
        ("~= 2.2", ">= 2.2, == 2.*"),
        ("~= 1.4.5", ">= 1.4.5, == 1.4.*"),

        ("~= 2.2.post3", ">= 2.2.post3, == 2.*"),        
        ("~= 1.4.5a4", ">= 1.4.5a4, == 1.4.*"),

        # for this one, we get 2.2.0 <= v < 2.3 instead of 2.2 <= v 2.3
        # ("~= 2.2.0", ">= 2.2.0, == 2.2.*"),
    ]
)
def test_compatible_versions(left, right):
    rl = VersionSpecifiers(left).to_pubgrub()
    rr = VersionSpecifiers(right).to_pubgrub()
    assert str(rl) == str(rr)