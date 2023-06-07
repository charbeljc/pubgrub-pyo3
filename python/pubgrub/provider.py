from abc import ABCMeta, abstractmethod
from typing import List, Type, TypeVar, Generic

P = TypeVar('P')
V = TypeVar('V')
R = TypeVar('R')

class AbstractDependencyProvider(Generic[P, V, R], metaclass=ABCMeta):
    def should_cancel(self):
        pass

    @abstractmethod
    def available_versions(self, package: P) -> list[V]:
        ...

    @abstractmethod
    def get_dependencies(
        self, package: P, version: V
    ) -> list[R]:
        ...
