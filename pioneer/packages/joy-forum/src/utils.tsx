import React from 'react';
import { Link } from 'react-router-dom';
import { Breadcrumb, Pagination as SuiPagination } from 'semantic-ui-react';
import styled from 'styled-components';

import { Category, CategoryId, Thread, ThreadId } from '@joystream/types/forum';
import { withForumCalls } from './calls';
import { withMulti } from '@polkadot/react-api';

export const ThreadsPerPage = 10;
export const RepliesPerPage = 10;

type PaginationProps = {
  currentPage?: number;
  totalItems: number;
  itemsPerPage?: number;
  onPageChange: (activePage?: string | number) => void;
};

export const Pagination = (p: PaginationProps) => {
  const { currentPage = 1, itemsPerPage = 20 } = p;
  const totalPages = Math.ceil(p.totalItems / itemsPerPage);

  return totalPages <= 1 ? null : (
    <SuiPagination
      firstItem={null}
      lastItem={null}
      defaultActivePage={currentPage}
      totalPages={totalPages}
      onPageChange={(_event, { activePage }) => p.onPageChange(activePage)}
    />
  );
};

type CategoryCrumbsProps = {
  categoryId?: CategoryId;
  category?: Category;
  threadId?: ThreadId;
  thread?: Thread;
  root?: boolean;
};

function InnerCategoryCrumb (p: CategoryCrumbsProps) {
  const { category } = p;

  if (category) {
    try {
      const url = `/forum/categories/${category.id.toString()}`;
      return <>
        {category.parent_id ? <CategoryCrumb categoryId={category.parent_id} /> : null}
        <Breadcrumb.Divider icon="right angle" />
        <Breadcrumb.Section as={Link} to={url}>{category.title}</Breadcrumb.Section>
      </>;
    } catch (err) {
      console.log('Failed to create a category breadcrumb', err);
    }
  }

  return null;
}

const CategoryCrumb = withMulti(
  InnerCategoryCrumb,
  withForumCalls<CategoryCrumbsProps>(
    ['categoryById', { propName: 'category', paramName: 'categoryId' }]
  )
);

function InnerThreadCrumb (p: CategoryCrumbsProps) {
  const { thread } = p;

  if (thread) {
    try {
      const url = `/forum/threads/${thread.id.toString()}`;
      return <>
        <CategoryCrumb categoryId={thread.category_id} />
        <Breadcrumb.Divider icon="right angle" />
        <Breadcrumb.Section as={Link} to={url}>{thread.title}</Breadcrumb.Section>
      </>;
    } catch (err) {
      console.log('Failed to create a thread breadcrumb', err);
    }
  }

  return null;
}

const ThreadCrumb = withMulti(
  InnerThreadCrumb,
  withForumCalls<CategoryCrumbsProps>(
    ['threadById', { propName: 'thread', paramName: 'threadId' }]
  )
);

const StyledBreadcrumbs = styled(Breadcrumb)`
  && {
    font-size: 1.3rem;
    line-height: 1.2;
  }
`;

export const CategoryCrumbs = ({ categoryId, threadId, root }: CategoryCrumbsProps) => {
  return (
    <StyledBreadcrumbs>
      <Breadcrumb.Section>Forum</Breadcrumb.Section>
      {!root && (
        <>
          <Breadcrumb.Divider icon="right angle" />
          <Breadcrumb.Section as={Link} to="/forum">Top categories</Breadcrumb.Section>
          <CategoryCrumb categoryId={categoryId} />
          <ThreadCrumb threadId={threadId} />
        </>
      )}
    </StyledBreadcrumbs>
  );
};

// It's used on such routes as:
//   /categories/:id
//   /categories/:id/edit
//   /threads/:id
//   /threads/:id/edit
export type UrlHasIdProps = {
  match: {
    params: {
      id: string;
    };
  };
};
